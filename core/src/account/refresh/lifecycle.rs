use super::{
    DeviceInfo,
    journal::{self, Begin, Intent},
    refresh_payload, within_jsk_interval,
};
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionStatus, SessionToken, credentials::LoginCredentials,
        identity::VerifiedAccount,
    },
    protocol::{
        api::{Api, ApiFuture, validate_response},
        http::MusicHttpClient,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub trait DeviceTokenProvider: Send + Sync {
    fn token(&self) -> ApiFuture<'_, String>;
}

/// Official PC M/290 has no SEND_YD_TOKEN producer; M/1368's timeout yields
/// this empty result. Native hosts need not simulate an unused five-second
/// bridge wait. This is distinct from the M/402 per-request checkToken.
pub struct DesktopDeviceToken;
impl DeviceTokenProvider for DesktopDeviceToken {
    fn token(&self) -> ApiFuture<'_, String> {
        Box::pin(async { Ok(String::new()) })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct RefreshStatus {
    pub pending: bool,
    pub last_success: Option<u64>,
}

#[derive(Clone, Copy)]
pub(crate) enum PendingReceipt {
    MissingPair,
    NeedsIdentity,
}

pub(crate) struct RefreshEvidence {
    pub pending: Option<PendingReceipt>,
    pub last_success: Option<u64>,
    pub has_pair: bool,
    pub last_login_at: Option<u64>,
}

fn has_pair(credentials: &LoginCredentials) -> bool {
    credentials.token.as_deref().is_some_and(|v| !v.is_empty())
        && credentials
            .refresh_token
            .as_deref()
            .is_some_and(|v| !v.is_empty())
}

fn fresh_receipt(credentials: &LoginCredentials, intent: &Intent) -> bool {
    has_pair(credentials)
        && credentials.rotation.is_some()
        && credentials.rotation != intent.before_rotation
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    MissingCredentials,
    NotDue,
    Refreshed,
    RecoveryRequired,
    /// No complete new pair is recorded; this does not invalidate the session.
    ReauthenticationRequired,
    NoPendingRefresh,
}

#[derive(Debug, Serialize)]
pub struct LogoutResult {
    pub local_signed_out: bool,
    pub remote_revoked: bool,
}

pub struct AccountLifecycle {
    session: Arc<AccountSession>,
    client: Arc<MusicHttpClient>,
    gate: tokio::sync::Mutex<()>,
    clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>,
}

impl AccountLifecycle {
    pub fn new(session: Arc<AccountSession>, client: Arc<MusicHttpClient>) -> Self {
        Self::with_clock(
            session,
            client,
            Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .map_err(|_| Error::State("system clock precedes UNIX epoch"))
            }),
        )
    }
    pub fn with_clock(
        session: Arc<AccountSession>,
        client: Arc<MusicHttpClient>,
        clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>,
    ) -> Self {
        Self {
            session,
            client,
            gate: tokio::sync::Mutex::new(()),
            clock,
        }
    }
    fn current(&self) -> Result<SessionToken> {
        let snapshot = self.session.snapshot()?;
        if snapshot.status != SessionStatus::Authenticated || snapshot.token.uid.is_none() {
            return Err(Error::State("an authenticated session is required"));
        }
        self.session.require_api(&snapshot.token, &self.api())?;
        Ok(snapshot.token)
    }
    fn api(&self) -> Arc<dyn Api> {
        self.client.clone()
    }

    pub fn refresh_status(&self) -> Result<RefreshStatus> {
        let token = self.current()?;
        let status = journal::load(
            &self.client.credentials().store(),
            token.uid.as_deref().ok_or(Error::StaleResponse)?,
        )?
        .map_or(RefreshStatus::default(), |i| RefreshStatus {
            pending: i.pending,
            last_success: i.last_success,
        });
        self.session.require_api(&token, &self.api())?;
        Ok(status)
    }

    pub(crate) fn refresh_evidence(&self) -> Result<RefreshEvidence> {
        let token = self.current()?;
        let credentials = self.client.credentials();
        let uid = token.uid.as_deref().ok_or(Error::StaleResponse)?;
        let store = credentials.store();
        let intent = journal::load(&store, uid)?;
        let legacy_pending =
            super::legacy::pending(store.get(uid, super::legacy::KIND, super::legacy::KEY)?)?;
        let credentials = credentials.login_credentials()?;
        let pending = intent
            .as_ref()
            .filter(|i| i.pending)
            .map(|i| {
                if fresh_receipt(&credentials, i) {
                    PendingReceipt::NeedsIdentity
                } else {
                    PendingReceipt::MissingPair
                }
            })
            .or(legacy_pending.then_some(PendingReceipt::MissingPair));
        self.session.require_api(&token, &self.api())?;
        Ok(RefreshEvidence {
            pending,
            last_success: intent.and_then(|i| i.last_success),
            has_pair: has_pair(&credentials),
            last_login_at: credentials.last_login_at,
        })
    }

    pub(crate) fn jsk_not_due(
        &self,
        last_login: Option<u64>,
        last_success: Option<u64>,
    ) -> Result<bool> {
        Ok(within_jsk_interval(
            (self.clock)()?,
            last_login,
            last_success,
        ))
    }

    async fn identity(&self, expected: &SessionToken) -> Result<Value> {
        self.session.require_api(expected, &self.api())?;
        let identity = self
            .session
            .run_for(expected, |api, _| async move {
                VerifiedAccount::read(&*api).await
            })
            .await?;
        let profile = identity.profile.clone();
        self.session
            .commit_refreshed_profile(expected, &self.api(), identity)?;
        Ok(profile)
    }

    pub async fn logout(&self, remote: bool) -> Result<LogoutResult> {
        let _guard = self.gate.lock().await;
        // Local-only logout also works after a 301 expired the account.
        let token = self.session.token()?;
        self.session.require_api(&token, &self.api())?;
        if remote {
            let (returned, _) = self
                .session
                .run(
                    |api, _| async move { api.call("/api/logout", json!({}), false, &[200]).await },
                )
                .await?;
            if returned != token {
                return Err(Error::StaleResponse);
            }
        }
        self.session
            .clear_current(&token, &self.api(), || self.client.credentials().clear())?;
        Ok(LogoutResult {
            local_signed_out: true,
            remote_revoked: remote,
        })
    }

    pub async fn refresh_legacy(&self) -> Result<Value> {
        let _guard = self.gate.lock().await;
        let token = self.current()?;
        let (returned, _) = self
            .session
            .run(|api, _| async move {
                api.call("/api/login/token/refresh", json!({}), false, &[200])
                    .await
            })
            .await?;
        if returned != token {
            return Err(Error::StaleResponse);
        }
        self.identity(&token).await
    }

    pub async fn reconcile_legacy(&self) -> Result<Value> {
        let _guard = self.gate.lock().await;
        let token = self.current()?;
        self.identity(&token).await
    }

    pub async fn refresh_jsk(
        &self,
        device: &DeviceInfo,
        provider: &dyn DeviceTokenProvider,
        force: bool,
    ) -> Result<RefreshOutcome> {
        let token = self.current()?;
        self.refresh_jsk_for(&token, device, provider, force).await
    }

    pub(crate) async fn refresh_jsk_for(
        &self,
        token: &SessionToken,
        device: &DeviceInfo,
        provider: &dyn DeviceTokenProvider,
        force: bool,
    ) -> Result<RefreshOutcome> {
        let _guard = self.gate.lock().await;
        let api = self.api();
        self.session.require_api(token, &api)?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing authenticated UID"))?;
        let credentials = self.client.credentials();
        let before = credentials.login_credentials()?;
        if !has_pair(&before) {
            return Ok(RefreshOutcome::MissingCredentials);
        }
        let store = credentials.store();
        if super::legacy::pending(store.get(uid, super::legacy::KIND, super::legacy::KEY)?)? {
            return Ok(RefreshOutcome::RecoveryRequired);
        }
        let previous = journal::load(&store, uid)?;
        if previous.as_ref().is_some_and(|i| i.pending) {
            return Ok(RefreshOutcome::RecoveryRequired);
        }
        let now = (self.clock)()?;
        if !force
            && within_jsk_interval(
                now,
                before.last_login_at,
                previous.and_then(|i| i.last_success),
            )
        {
            return Ok(RefreshOutcome::NotDue);
        }
        if device.device_id != credentials.device_id()? {
            return Err(Error::InvalidInput(
                "refresh device identity does not match this session",
            ));
        }
        let music_u = before.token.as_deref().ok_or(Error::CredentialChanged)?;
        self.client
            .require_jsk_renewal_scope(&before.revision, music_u)?;
        let payload = refresh_payload(&before, &json!(uid), device, &provider.token().await?)?;
        self.session.require_api(token, &api)?;
        if credentials.login_credentials()?.revision != before.revision {
            return Err(Error::CredentialChanged);
        }
        let intent = match journal::begin(&store, uid, now, force, before.rotation.clone())? {
            Begin::Started(intent) => intent,
            Begin::NotDue => return Ok(RefreshOutcome::NotDue),
            Begin::RecoveryRequired => return Ok(RefreshOutcome::RecoveryRequired),
        };
        const PATH: &str = "/api/login/token/refresh/v1";
        // M/1368 calls the native bridge and handles a rejected periodic
        // refresh itself; it does not enter the ordinary 301 business path.
        let allowed = || self.session.require_api(token, &api);
        let mut dispatch = journal::Dispatch::new(&store, uid, &intent);
        let result = self
            .client
            .request_jsk_renewal(
                payload,
                &before.revision,
                music_u,
                &allowed,
                &dispatch.dispatched,
            )
            .await
            .and_then(|response| validate_response(PATH, response, &[200]));
        dispatch.release_unsent()?;
        self.session.require_api(token, &api)?;
        result?;
        self.finish_jsk(token, &intent).await
    }

    async fn finish_jsk(&self, token: &SessionToken, intent: &Intent) -> Result<RefreshOutcome> {
        let credentials = self.client.credentials();
        let after = credentials.login_credentials()?;
        if !fresh_receipt(&after, intent) {
            return Ok(RefreshOutcome::ReauthenticationRequired);
        }
        // Identity reconciliation is a read only; an ordinary 301
        // interceptor could otherwise issue a second refresh request.
        let api = self.api();
        let music_u = after.token.as_deref().ok_or(Error::CredentialChanged)?;
        let allowed = || self.session.require_api(token, &api);
        allowed()?;
        let identity = self
            .client
            .request_jsk_renewal_identity(&after.revision, music_u, &allowed)
            .await
            .and_then(|reply| VerifiedAccount::from_reply(&reply));
        allowed()?;
        let identity = identity?;
        let current = credentials.login_credentials()?;
        if !fresh_receipt(&current, intent) || current.rotation != after.rotation {
            return Err(Error::CredentialChanged);
        }
        self.client
            .require_jsk_renewal_scope(&current.revision, music_u)?;
        self.session
            .commit_refreshed_profile(token, &api, identity)?;
        journal::complete(
            &credentials.store(),
            token.uid.as_deref().ok_or(Error::StaleResponse)?,
            intent,
            (self.clock)()?,
        )?;
        Ok(RefreshOutcome::Refreshed)
    }

    /// Reopen recovery only reads identity and the committed rotation receipt.
    /// No new refresh is sent when the previous request has an unknown outcome.
    pub async fn reconcile_jsk(&self) -> Result<RefreshOutcome> {
        let token = self.current()?;
        self.reconcile_jsk_for(&token).await
    }

    pub(crate) async fn reconcile_jsk_for(&self, token: &SessionToken) -> Result<RefreshOutcome> {
        let _guard = self.gate.lock().await;
        self.session.require_api(token, &self.api())?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing authenticated UID"))?;
        let Some(intent) =
            journal::load(&self.client.credentials().store(), uid)?.filter(|i| i.pending)
        else {
            return Ok(RefreshOutcome::NoPendingRefresh);
        };
        self.finish_jsk(token, &intent).await
    }
}
