//! Separate, process-local Interface authorization for external playlist import.
//! The archived type=3 QR proof used this origin before the independent PC QR;
//! the generic desktop browser SSO is not an authorization for this target.
#[cfg(test)]
mod tests;
use super::MusicClient;
use crate::{
    Error, Result,
    account::{QrLogin, QrStatus, SessionToken, credentials::Credentials},
    protocol::{
        api::{Api, identifier},
        http::{
            ApiOrigin, EapiStyle, MusicHttpClient, RequestOptions, ReqwestBackend, RoutingPolicy,
            WireRoute,
        },
    },
    storage::StateStore,
};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

const ORIGIN: ApiOrigin = ApiOrigin::Interface;
const IMPORT_PATH: &str = "/api/playlist/import/name/task/create";
const IDENTITY_PATH: &str = "/api/w/nuser/account/get";
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalImportAuthState {
    #[default]
    Idle,
    Creating,
    WaitingScan,
    WaitingConfirmation,
    IdentityPending,
    Authorized,
    Expired,
    Challenge,
    WrongAccount,
    Unconfirmed,
}

impl ExternalImportAuthState {
    fn waiting(self) -> bool {
        matches!(self, Self::WaitingScan | Self::WaitingConfirmation)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportAuthView {
    pub epoch: String,
    pub operation_id: Option<String>,
    pub state: ExternalImportAuthState,
    pub busy: bool,
    pub poll_after_ms: Option<u64>,
    pub modules: Vec<Vec<bool>>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalImportAuthAction {
    Begin,
    Poll { operation_id: String },
    Reconcile { operation_id: String },
    Cancel { operation_id: String },
}

#[derive(Default)]
pub(super) struct ExternalImportAuthHost(Mutex<Option<Arc<Attempt>>>);

struct Attempt {
    id: String,
    owner: SessionToken,
    credentials: Arc<Credentials>,
    client: Arc<MusicHttpClient>,
    qr: QrLogin,
    state: Mutex<AttemptState>,
}

#[derive(Default)]
struct AttemptState {
    phase: ExternalImportAuthState,
    active: bool,
    busy: bool,
    modules: Vec<Vec<bool>>,
    next_poll: Option<Instant>,
    verified_revision: Option<String>,
}

struct Flight<'a> {
    attempt: &'a Attempt,
    interrupted: ExternalImportAuthState,
    finished: bool,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        if !self.finished
            && let Ok(mut state) = self.attempt.state.lock()
            && state.active
            && state.busy
        {
            state.busy = false;
            state.phase = self.interrupted;
            state.modules.clear();
            let _ = self.attempt.qr.cancel();
        }
    }
}

impl Flight<'_> {
    fn finish(mut self, phase: ExternalImportAuthState, revision: Option<String>) -> Result<()> {
        self.attempt.finish(phase, revision)?;
        self.finished = true;
        Ok(())
    }
}

impl Attempt {
    fn lock(&self) -> Result<MutexGuard<'_, AttemptState>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    fn view(&self, epoch: &str) -> Result<ExternalImportAuthView> {
        let state = self.lock()?;
        if !state.active {
            return Err(Error::StaleResponse);
        }
        Ok(ExternalImportAuthView {
            epoch: epoch.into(),
            operation_id: Some(self.id.clone()),
            state: state.phase,
            busy: state.busy,
            poll_after_ms: state
                .phase
                .waiting()
                .then_some(POLL_INTERVAL.as_millis() as u64),
            modules: if state.phase == ExternalImportAuthState::WaitingScan {
                state.modules.clone()
            } else {
                Vec::new()
            },
        })
    }

    fn begin_flight(
        &self,
        expected: ExternalImportAuthState,
        interrupted: ExternalImportAuthState,
    ) -> Result<Flight<'_>> {
        let mut state = self.lock()?;
        if !state.active {
            return Err(Error::StaleResponse);
        }
        if state.busy || state.phase != expected {
            return Err(Error::State(
                "external import authorization action is not available",
            ));
        }
        state.busy = true;
        Ok(Flight {
            attempt: self,
            interrupted,
            finished: false,
        })
    }

    fn finish(&self, phase: ExternalImportAuthState, revision: Option<String>) -> Result<()> {
        use ExternalImportAuthState as Phase;
        let mut state = self.lock()?;
        if !state.active {
            return Err(Error::StaleResponse);
        }
        if !matches!(
            phase,
            Phase::WaitingScan
                | Phase::WaitingConfirmation
                | Phase::IdentityPending
                | Phase::Authorized
        ) {
            self.credentials.clear()?;
        }
        if !phase.waiting() {
            self.qr.cancel()?;
        }
        state.phase = phase;
        state.verified_revision = revision;
        state.next_poll = phase.waiting().then(|| Instant::now() + POLL_INTERVAL);
        if phase != Phase::WaitingScan {
            state.modules.clear();
        }
        state.busy = false;
        Ok(())
    }

    fn revoke(&self) -> Result<()> {
        let mut state = self.lock()?;
        state.active = false;
        state.modules.clear();
        state.verified_revision = None;
        self.qr.cancel()?;
        // Even a native holder of the previously returned Arc cannot start a
        // new authenticated request after cancellation or account change.
        self.credentials.clear()
    }

    fn matching_credentials(
        &self,
    ) -> Result<Option<crate::account::credentials::RequestCredentials>> {
        let identity = self
            .credentials
            .request_credentials(&ORIGIN.request_url(WireRoute::Eapi, IDENTITY_PATH)?)?;
        let target = self
            .credentials
            .request_credentials(&ORIGIN.request_url(WireRoute::Weapi, IMPORT_PATH)?)?;
        if identity.revision != target.revision {
            return Err(Error::CredentialChanged);
        }
        Ok((identity.unique_music_u().is_some()
            && identity.unique_music_u() == target.unique_music_u())
        .then_some(target))
    }

    fn confirmed_credentials(&self) -> Result<Option<Arc<Credentials>>> {
        let mut state = self.lock()?;
        if !state.active || state.phase != ExternalImportAuthState::Authorized || state.busy {
            return Ok(None);
        }
        let current = self.matching_credentials()?;
        if current.as_ref().map(|snapshot| &snapshot.revision) == state.verified_revision.as_ref()
            && current.is_some()
        {
            return Ok(Some(self.credentials.clone()));
        }
        // Later cookie changes need a new identity read; do not borrow the old proof.
        state.verified_revision = None;
        state.phase = if current.is_some() {
            ExternalImportAuthState::IdentityPending
        } else {
            self.credentials.clear()?;
            ExternalImportAuthState::Unconfirmed
        };
        Ok(None)
    }
}

impl ExternalImportAuthHost {
    fn slot(&self, owner: &SessionToken) -> Result<MutexGuard<'_, Option<Arc<Attempt>>>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(attempt) = slot.as_ref().filter(|attempt| attempt.owner != *owner) {
            // A late call with an older token must never purge a newer candidate.
            if attempt.owner.generation >= owner.generation {
                return Err(Error::StaleResponse);
            }
            attempt.revoke()?;
            *slot = None;
        }
        Ok(slot)
    }

    fn get(&self, owner: &SessionToken, id: &str) -> Result<Arc<Attempt>> {
        self.slot(owner)?
            .as_ref()
            .filter(|attempt| attempt.id == id)
            .cloned()
            .ok_or(Error::StaleResponse)
    }
}

impl MusicClient {
    pub fn external_import_auth_view(&self, epoch: &str) -> Result<ExternalImportAuthView> {
        let owner = self.playlist_token(epoch)?;
        // A rotated or expired in-memory cookie must never leave the UI showing
        // an active authorization which the importer can no longer use.
        self.external_import_credentials(&owner)?;
        let attempt = self.external_import_auth.slot(&owner)?.clone();
        self.session.check_generation(&owner)?;
        match attempt {
            Some(attempt) => attempt.view(epoch),
            None => Ok(ExternalImportAuthView {
                epoch: epoch.into(),
                operation_id: None,
                state: ExternalImportAuthState::Idle,
                busy: false,
                poll_after_ms: None,
                modules: Vec::new(),
            }),
        }
    }

    pub(super) fn external_import_credentials(
        &self,
        owner: &SessionToken,
    ) -> Result<Option<Arc<Credentials>>> {
        self.session.with_token(owner, |_| Ok(()))?;
        let attempt = self.external_import_auth.slot(owner)?.clone();
        let credentials = attempt
            .map(|attempt| attempt.confirmed_credentials())
            .transpose()?
            .flatten();
        self.session.check_generation(owner)?;
        Ok(credentials)
    }

    pub async fn external_import_auth_command(
        &self,
        epoch: &str,
        action: ExternalImportAuthAction,
    ) -> Result<ExternalImportAuthView> {
        let owner = self.playlist_token(epoch)?;
        let operation = match action {
            ExternalImportAuthAction::Begin => Some(self.begin_external_import_qr(&owner).await?),
            ExternalImportAuthAction::Poll { operation_id } => {
                let attempt = self.external_import_auth.get(&owner, &operation_id)?;
                self.poll_external_import_qr(&attempt).await?;
                Some(operation_id)
            }
            ExternalImportAuthAction::Reconcile { operation_id } => {
                use ExternalImportAuthState::IdentityPending;
                let attempt = self.external_import_auth.get(&owner, &operation_id)?;
                let flight = attempt.begin_flight(IdentityPending, IdentityPending)?;
                let (phase, revision) = self.external_import_identity(&attempt).await?;
                flight.finish(phase, revision)?;
                Some(operation_id)
            }
            ExternalImportAuthAction::Cancel { operation_id } => {
                let mut slot = self.external_import_auth.slot(&owner)?;
                let attempt = slot
                    .as_ref()
                    .filter(|attempt| attempt.id == operation_id)
                    .ok_or(Error::StaleResponse)?;
                attempt.revoke()?;
                *slot = None;
                None
            }
        };
        let view = self.external_import_auth_view(epoch)?;
        if view.operation_id != operation {
            return Err(Error::StaleResponse);
        }
        Ok(view)
    }

    async fn begin_external_import_qr(&self, owner: &SessionToken) -> Result<String> {
        use ExternalImportAuthState as Phase;
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::volatile_credentials()?),
            ORIGIN,
        )?);
        let backend = self.auth.backend.clone().map_or_else(
            || {
                Ok::<Arc<dyn crate::protocol::http::HttpBackend>, Error>(Arc::new(
                    ReqwestBackend::new(Duration::from_secs(20))?,
                ))
            },
            Ok,
        )?;
        let client = Arc::new(MusicHttpClient::with_backend(
            credentials.clone(),
            self.auth.profile.clone(),
            RequestOptions {
                style: EapiStyle::Native,
                routing: RoutingPolicy::OfficialEapi,
                ..Default::default()
            },
            None,
            backend,
        )?);
        let attempt = Arc::new(Attempt {
            id: Uuid::new_v4().simple().to_string(),
            owner: owner.clone(),
            credentials,
            qr: QrLogin::new(client.clone()),
            client,
            state: Mutex::new(AttemptState {
                phase: Phase::Creating,
                active: true,
                ..Default::default()
            }),
        });
        let flight = attempt.begin_flight(Phase::Creating, Phase::Unconfirmed)?;
        {
            let mut slot = self.external_import_auth.slot(owner)?;
            self.session.check_generation(owner)?;
            if let Some(old) = slot.as_ref() {
                if old.lock()?.busy {
                    return Err(Error::State("external import authorization is busy"));
                }
                old.revoke()?;
            }
            *slot = Some(attempt.clone());
        }
        let result = attempt.qr.begin().await;
        self.session.check_generation(owner)?;
        match result.and_then(|key| qr_matrix(&key)) {
            Ok(modules) => {
                {
                    let mut state = attempt.lock()?;
                    if !state.active {
                        return Err(Error::StaleResponse);
                    }
                    state.modules = modules;
                }
                flight.finish(Phase::WaitingScan, None)?;
                Ok(attempt.id.clone())
            }
            Err(error) => {
                flight.finish(Phase::Unconfirmed, None)?;
                Err(error)
            }
        }
    }

    async fn poll_external_import_qr(&self, attempt: &Attempt) -> Result<()> {
        use ExternalImportAuthState as Phase;
        let waiting = {
            let state = attempt.lock()?;
            if !state.active {
                return Err(Error::StaleResponse);
            }
            if !state.phase.waiting() || state.busy {
                return Err(Error::State("external import QR is not waiting"));
            }
            if state
                .next_poll
                .is_some_and(|deadline| Instant::now() < deadline)
            {
                return Ok(());
            }
            state.phase
        };
        let flight = attempt.begin_flight(waiting, Phase::IdentityPending)?;
        self.session.check_generation(&attempt.owner)?;
        let result = attempt.qr.poll(None).await;
        self.session.check_generation(&attempt.owner)?;
        let (phase, revision) = match result {
            Ok(QrStatus::Authorized) => self.external_import_identity(attempt).await?,
            Ok(QrStatus::WaitingScan) if waiting == Phase::WaitingScan => {
                (Phase::WaitingScan, None)
            }
            Ok(QrStatus::WaitingConfirmation | QrStatus::WaitingScan) => {
                (Phase::WaitingConfirmation, None)
            }
            Ok(QrStatus::Expired) => (Phase::Expired, None),
            Ok(QrStatus::Challenge) => (Phase::Challenge, None),
            // The transport could already have accepted a login and committed
            // scoped Set-Cookie on a truncated body. Never poll that QR again.
            Err(_) if attempt.qr.snapshot()?.status != QrStatus::Error => {
                (Phase::IdentityPending, None)
            }
            _ => (Phase::Unconfirmed, None),
        };
        flight.finish(phase, revision)
    }

    async fn external_import_identity(
        &self,
        attempt: &Attempt,
    ) -> Result<(ExternalImportAuthState, Option<String>)> {
        use ExternalImportAuthState as Phase;
        self.session.check_generation(&attempt.owner)?;
        {
            let mut state = attempt.lock()?;
            if !state.active {
                return Err(Error::StaleResponse);
            }
            state.phase = Phase::IdentityPending;
            state.modules.clear();
        }
        let Some(before) = attempt.matching_credentials()? else {
            return Ok((Phase::Unconfirmed, None));
        };
        let identity = attempt.client.account().await;
        self.session.check_generation(&attempt.owner)?;
        let profile = match identity {
            Ok(profile) => profile,
            Err(error) if error.is_service_code(301) => return Ok((Phase::Unconfirmed, None)),
            Err(_) => return Ok((Phase::IdentityPending, None)),
        };
        let uid = identifier(&profile["userId"])?;
        if Some(&uid) != attempt.owner.uid.as_ref() {
            return Ok((Phase::WrongAccount, None));
        }
        let Some(after) = attempt.matching_credentials()? else {
            return Ok((Phase::Unconfirmed, None));
        };
        if before.unique_music_u() != after.unique_music_u() {
            return Ok((Phase::IdentityPending, None));
        }
        Ok((Phase::Authorized, Some(after.revision)))
    }
}

fn qr_matrix(key: &str) -> Result<Vec<Vec<bool>>> {
    let mut url =
        url::Url::parse("https://music.163.com/login").map_err(|_| Error::Protocol("QR URL"))?;
    url.query_pairs_mut().append_pair("codekey", key);
    let qr = QrCode::encode_text(url.as_str(), QrCodeEcc::Medium)
        .map_err(|_| Error::Response("QR payload exceeds limit"))?;
    Ok((0..qr.size())
        .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
        .collect())
}
