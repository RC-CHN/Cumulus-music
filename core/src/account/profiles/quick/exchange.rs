//! One-shot JSK exchanges with fresh, target-bound receipt stores.
use super::{History, PATH, ProfileRegistry, QuickSavedTarget};
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken,
        identity::VerifiedAccount,
        login::{LoginStatus, classify},
        profiles::LoginCandidate,
    },
    protocol::{
        api::{Api, identifier},
        context::RequestContext,
        http::{ApiOrigin, ClientProfile, HttpBackend, MusicHttpClient, RequestOptions, WireRoute},
    },
    storage::OperationLock,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    any::Any,
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::watch;

/// Native dependencies only. A renderer cannot supply credentials or route.
pub struct QuickLoginTransport {
    pub profile: ClientProfile,
    pub options: RequestOptions,
    pub context: Option<Arc<RequestContext>>,
    pub backend: Arc<dyn HttpBackend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerQuickLoginPhase {
    Ready,
    Exchanging,
    CheckingIdentity,
    IdentityRequired,
    ResultUnknown,
    VerificationRequired,
    Rejected,
    Authenticated,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerQuickLoginView {
    pub phase: ServerQuickLoginPhase,
    pub can_reconcile: bool,
    pub status: Option<LoginStatus>,
}

struct Request {
    history: History,
    payload: Value,
}

struct State {
    phase: ServerQuickLoginPhase,
    request: Option<Request>,
    receipt: Option<String>,
    status: Option<LoginStatus>,
}

const SWITCH_PATH: &str = "/api/login/switchuser";

struct ActiveSource {
    api: Arc<MusicHttpClient>,
    revision: String,
    music_u: String,
}

pub struct ServerQuickLogin {
    session: Arc<AccountSession>,
    base: SessionToken,
    source: Arc<dyn Api>,
    active: Option<ActiveSource>,
    candidate: LoginCandidate,
    state: Mutex<State>,
    cancelled: watch::Sender<bool>,
}

impl ProfileRegistry {
    pub async fn begin_guest_server_quick_for(
        self: &Arc<Self>,
        session: Arc<AccountSession>,
        expected: &SessionToken,
        saved_id: &str,
        transport: QuickLoginTransport,
    ) -> Result<ServerQuickLogin> {
        if expected.uid.is_some() {
            return Err(Error::State("server quick login requires a guest session"));
        }
        self.begin_quick_exchange_for(session, expected, saved_id, transport)
            .await
    }

    /// Active switch keeps the current credential store untouched. The
    /// selected target must be a different validated saved account.
    pub async fn begin_active_server_switch_for(
        self: &Arc<Self>,
        session: Arc<AccountSession>,
        expected: &SessionToken,
        saved_id: &str,
        transport: QuickLoginTransport,
    ) -> Result<ServerQuickLogin> {
        if expected.uid.is_none() {
            return Err(Error::State("server switch requires an active account"));
        }
        self.begin_quick_exchange_for(session, expected, saved_id, transport)
            .await
    }

    async fn begin_quick_exchange_for(
        self: &Arc<Self>,
        session: Arc<AccountSession>,
        expected: &SessionToken,
        saved_id: &str,
        transport: QuickLoginTransport,
    ) -> Result<ServerQuickLogin> {
        if !super::super::id(saved_id) {
            return Err(Error::InvalidInput("invalid saved account identifier"));
        }
        let source = session.native_api(expected)?;
        // M1368:4455–4482 revalidates the history at the actual login action.
        let (history, eligibility) = self.checked_quick_history(&session, expected).await?;
        let (active, device_id) = if expected.uid.is_some() {
            let current =
                history
                    .entries
                    .iter()
                    .find(|entry| entry.current)
                    .ok_or(Error::State(
                        "active account is not in the saved JSK history",
                    ))?;
            if history
                .selection
                .as_ref()
                .map(|value| value.session_id.as_str())
                != Some(current.saved.session_id.as_str())
            {
                return Err(Error::StaleResponse);
            }
            let source: Arc<dyn Any + Send + Sync> = source.clone();
            let api = source
                .downcast::<MusicHttpClient>()
                .map_err(|_| Error::State("native JSK switch source unavailable"))?;
            let url = ApiOrigin::Pc.request_url(WireRoute::Eapi, SWITCH_PATH)?;
            let snapshot = api.credentials().request_credentials(&url)?;
            let music_u = snapshot
                .unique_music_u()
                .ok_or(Error::State(
                    "active account has no unique scoped switch credential",
                ))?
                .to_owned();
            if snapshot.revision != current.credential.revision
                || music_u != current.credential.music_u
            {
                return Err(Error::CredentialChanged);
            }
            (
                Some(ActiveSource {
                    api,
                    revision: snapshot.revision,
                    music_u,
                }),
                Some(snapshot.device_id),
            )
        } else {
            (None, None)
        };
        if !eligibility
            .accounts
            .iter()
            .any(|row| row.id == saved_id && row.eligible && !row.current)
        {
            return Err(Error::State(
                "saved account is not eligible for server quick login",
            ));
        }
        let target = history
            .entries
            .iter()
            .find(|entry| entry.saved.session_id == saved_id)
            .ok_or(Error::StaleResponse)?;
        // M1368:9475–9491 uses a JavaScript Number for targetUserId. A UID
        // outside its exact integer range remains supported by local restore.
        let numeric_target = target
            .saved
            .uid
            .parse::<u64>()
            .ok()
            .filter(|value| {
                (1..=9_007_199_254_740_991).contains(value) && value.to_string() == target.saved.uid
            })
            .ok_or(Error::InvalidInput(
                "saved account ID exceeds JSK numeric range",
            ))?;
        let binding = QuickSavedTarget {
            saved: target.saved.clone(),
            credential_revision: target.credential.revision.clone(),
            offer: target.offer.clone(),
        };
        let registry = self.clone();
        let current_uid = expected.uid.clone().unwrap_or_default();
        let (candidate, request) = tokio::task::spawn_blocking(move || {
            let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, PATH)?;
            registry.check_quick_history(&history, &target)?;
            let mut payload = registry.quick_payload(&history)?;
            payload["targetUserId"] = json!(numeric_target);
            payload["currentUserId"] = json!(current_uid);
            let mut candidate = registry.candidate_with_backend(
                transport.profile,
                transport.options,
                transport.context,
                transport.backend,
            )?;
            if candidate.expected != history.selection {
                return Err(Error::StaleResponse);
            }
            if let Some(device_id) = device_id {
                candidate.credentials().seed_switch_device(&device_id)?;
            }
            candidate.remember(binding.saved.auto_login);
            candidate.expected_uid = Some(binding.saved.uid.clone());
            candidate.quick_target = Some(binding);
            Ok::<_, Error>((candidate, Request { history, payload }))
        })
        .await
        .map_err(|_| Error::State("quick candidate worker stopped"))??;
        session.require_api(expected, &source)?;
        let (cancelled, _) = watch::channel(false);
        Ok(ServerQuickLogin {
            session,
            base: expected.clone(),
            source,
            active,
            candidate,
            cancelled,
            state: Mutex::new(State {
                phase: ServerQuickLoginPhase::Ready,
                request: Some(request),
                receipt: None,
                status: None,
            }),
        })
    }
}

impl ServerQuickLogin {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    pub fn snapshot(&self) -> Result<ServerQuickLoginView> {
        let state = self.lock()?;
        Ok(ServerQuickLoginView {
            phase: state.phase,
            can_reconcile: state.phase == ServerQuickLoginPhase::IdentityRequired
                && state.receipt.is_some(),
            status: state.status,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        let mut state = self.lock()?;
        if state.phase != ServerQuickLoginPhase::Authenticated {
            state.phase = ServerQuickLoginPhase::Cancelled;
            state.request = None;
            state.receipt = None;
            self.cancelled.send_replace(true);
            drop(state);
            // Clear only the unselected candidate. Incrementing its credential
            // revision also prevents a racing old HTTP response from writing.
            self.clear_unselected();
        }
        Ok(())
    }

    async fn scoped<T>(&self, work: impl Future<Output = Result<T>>) -> Result<T> {
        let mut cancelled = self.cancelled.subscribe();
        let mut account = self.session.subscribe()?;
        if *cancelled.borrow() {
            return Err(Error::Cancelled);
        }
        self.check_source()?;
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::Cancelled),
            _ = account.changed() => Err(Error::StaleResponse),
            result = work => result,
        }
    }

    pub async fn execute(&self) -> Result<ServerQuickLoginView> {
        let (_lock, request) = {
            let mut state = self.lock()?;
            if state.phase != ServerQuickLoginPhase::Ready {
                drop(state);
                return self.snapshot();
            }
            let offer_lock = self.candidate.registry.quick_offer_lock()?;
            let guard = self.exchange_lock()?;
            let request = state
                .request
                .take()
                .ok_or(Error::State("missing quick login request"))?;
            state.phase = ServerQuickLoginPhase::Exchanging;
            ((guard, offer_lock), request)
        };
        let _interrupted = Interrupted(&self.state);
        let registry = self.candidate.registry.clone();
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, PATH)?;
        let history = request.history;
        let prepared = self
            .scoped(async move {
                tokio::task::spawn_blocking(move || registry.check_quick_history(&history, &target))
                    .await
                    .map_err(|_| Error::State("quick history worker stopped"))?
            })
            .await;
        if let Err(error) = prepared {
            let mut state = self.lock()?;
            if state.phase == ServerQuickLoginPhase::Exchanging {
                state.phase = ServerQuickLoginPhase::Rejected;
            }
            return Err(error);
        }
        let client = self.candidate.client();
        let response = self
            .scoped(async {
                match &self.active {
                    Some(source) => {
                        client
                            .request_jsk_active_switch(
                                &source.api,
                                &source.revision,
                                request.payload,
                            )
                            .await
                    }
                    None => client.request_jsk_guest_quick(request.payload).await,
                }
            })
            .await;
        let receipt = self.fresh_receipt()?;
        let read_now = {
            let mut state = self.lock()?;
            if state.phase != ServerQuickLoginPhase::Exchanging {
                return Err(Error::Cancelled);
            }
            self.check_source()?;
            match response.map(normalize_code) {
                Ok(value)
                    if value["code"] == 200
                        && value["data"]["success"] != false
                        && value["data"] != false =>
                {
                    state.receipt = receipt;
                    state.phase = if state.receipt.is_some() {
                        ServerQuickLoginPhase::CheckingIdentity
                    } else {
                        ServerQuickLoginPhase::ResultUnknown
                    };
                }
                Ok(value) if value["code"].as_i64().is_some() => {
                    let status = classify(&value);
                    state.status = Some(status);
                    state.phase = match status {
                        LoginStatus::Rejected
                        | LoginStatus::RateLimited
                        | LoginStatus::AccountDisabled
                        | LoginStatus::Authorized => ServerQuickLoginPhase::Rejected,
                        _ => ServerQuickLoginPhase::VerificationRequired,
                    };
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(Error::StaleResponse | Error::CredentialChanged) => {
                    return Err(Error::StaleResponse);
                }
                Err(Error::RequestNotSent(_)) => state.phase = ServerQuickLoginPhase::Rejected,
                // A 2xx response can have a real scoped cookie and a lost
                // body. Recover via identity only; never send quick again.
                _ => {
                    state.receipt = receipt;
                    state.phase = if state.receipt.is_some() {
                        ServerQuickLoginPhase::IdentityRequired
                    } else {
                        ServerQuickLoginPhase::ResultUnknown
                    };
                }
            }
            state.phase == ServerQuickLoginPhase::CheckingIdentity
        };
        if read_now {
            self.identity().await?;
        }
        self.snapshot()
    }

    pub async fn reconcile(&self) -> Result<ServerQuickLoginView> {
        let _lock = {
            let mut state = self.lock()?;
            if state.phase != ServerQuickLoginPhase::IdentityRequired || state.receipt.is_none() {
                drop(state);
                return self.snapshot();
            }
            let guard = self.exchange_lock()?;
            state.phase = ServerQuickLoginPhase::CheckingIdentity;
            guard
        };
        let _interrupted = Interrupted(&self.state);
        self.identity().await?;
        self.snapshot()
    }

    async fn identity(&self) -> Result<()> {
        let api = self.candidate.api();
        let before = self.fresh_receipt()?;
        {
            let state = self.lock()?;
            if state.phase != ServerQuickLoginPhase::CheckingIdentity {
                return Err(Error::Cancelled);
            }
            if state.receipt.is_none() || state.receipt != before {
                return Err(Error::CredentialChanged);
            }
        }
        let identity = self.scoped(VerifiedAccount::read(&*api)).await;
        let mut state = self.lock()?;
        if state.phase != ServerQuickLoginPhase::CheckingIdentity {
            return Err(Error::Cancelled);
        }
        self.check_source()?;
        if self.fresh_receipt()? != before {
            state.receipt = None;
            state.phase = ServerQuickLoginPhase::ResultUnknown;
            return Err(Error::CredentialChanged);
        }
        let identity = match identity {
            Ok(value) => value,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(Error::StaleResponse) => return Err(Error::StaleResponse),
            Err(_) => {
                state.phase = ServerQuickLoginPhase::IdentityRequired;
                return Ok(());
            }
        };
        if identifier(&identity.profile["userId"]).as_deref().ok()
            != self.candidate.expected_uid.as_deref()
        {
            state.phase = ServerQuickLoginPhase::Rejected;
            state.receipt = None;
            return Ok(());
        }
        self.candidate.adopt(&self.session, &self.base, identity)?;
        state.phase = ServerQuickLoginPhase::Authenticated;
        state.receipt = None;
        Ok(())
    }

    fn check_source(&self) -> Result<()> {
        self.session.require_api(&self.base, &self.source)?;
        if let Some(source) = &self.active {
            let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, SWITCH_PATH)?;
            let snapshot = source.api.credentials().request_credentials(&target)?;
            if snapshot.revision != source.revision
                || snapshot.unique_music_u() != Some(source.music_u.as_str())
            {
                return Err(Error::CredentialChanged);
            }
        }
        Ok(())
    }

    fn exchange_lock(&self) -> Result<OperationLock> {
        OperationLock::acquire(
            self.candidate.registry.store.directory(),
            "server-quick-login-v1",
            "account",
        )
    }

    fn fresh_receipt(&self) -> Result<Option<String>> {
        let credentials = self.candidate.credentials();
        let refresh = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/login/token/refresh/v1")?;
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/w/nuser/account/get")?;
        let refresh_scope = credentials.request_credentials(&refresh)?;
        let identity_scope = credentials.request_credentials(&target)?;
        let Some(token) = identity_scope.unique_music_u() else {
            return Ok(None);
        };
        if identity_scope.revision != refresh_scope.revision
            || refresh_scope.unique_music_u() != Some(token)
            || self
                .active
                .as_ref()
                .is_some_and(|source| token == source.music_u)
        {
            return Ok(None);
        }
        Ok(Some(token.to_owned()))
    }

    fn clear_unselected(&self) {
        if self.candidate.registry.selection().is_ok_and(|selection| {
            selection.is_none_or(|selected| selected.session_id != self.candidate.id)
        }) {
            let _ = self.candidate.credentials().clear();
        }
    }
}

impl Drop for ServerQuickLogin {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.phase != ServerQuickLoginPhase::Authenticated {
            self.clear_unselected();
        }
    }
}

fn normalize_code(mut response: Value) -> Value {
    if let Some(code) = response["code"]
        .as_str()
        .and_then(|code| code.parse::<i64>().ok())
    {
        response["code"] = json!(code);
    }
    response
}

struct Interrupted<'a>(&'a Mutex<State>);
impl Drop for Interrupted<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock()
            && matches!(
                state.phase,
                ServerQuickLoginPhase::Exchanging | ServerQuickLoginPhase::CheckingIdentity
            )
        {
            state.phase = if state.receipt.is_some() {
                ServerQuickLoginPhase::IdentityRequired
            } else {
                ServerQuickLoginPhase::ResultUnknown
            };
        }
    }
}
