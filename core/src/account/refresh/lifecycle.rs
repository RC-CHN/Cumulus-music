use super::{
    DeviceInfo,
    journal::{self, Begin, Intent},
    refresh_payload,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    MissingCredentials,
    NotDue,
    Refreshed,
    RecoveryRequired,
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

    async fn identity(&self, expected: &SessionToken) -> Result<Value> {
        self.session.require_api(expected, &self.api())?;
        let profile = self
            .session
            .run_for(expected, |api, _| async move { api.account().await })
            .await?;
        self.session
            .commit_refreshed_profile(expected, &self.api(), profile.clone())?;
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
        let _guard = self.gate.lock().await;
        let token = self.current()?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing authenticated UID"))?;
        let credentials = self.client.credentials();
        let before = credentials.login_credentials()?;
        if before.token.as_deref().is_none_or(str::is_empty)
            || before.refresh_token.as_deref().is_none_or(str::is_empty)
        {
            return Ok(RefreshOutcome::MissingCredentials);
        }
        let store = credentials.store();
        if journal::load(&store, uid)?.is_some_and(|i| i.pending) {
            return Ok(RefreshOutcome::RecoveryRequired);
        }
        let now = (self.clock)()?;
        if !force
            && journal::load(&store, uid)?
                .and_then(|i| i.last_success)
                .is_some_and(|last| last != 0 && now < last.saturating_add(86400))
        {
            return Ok(RefreshOutcome::NotDue);
        }
        if device.device_id != credentials.device_id()? {
            return Err(Error::InvalidInput(
                "refresh device identity does not match this session",
            ));
        }
        let payload = refresh_payload(&before, &json!(uid), device, &provider.token().await?)?;
        self.session.require_api(&token, &self.api())?;
        if credentials.login_credentials()?.revision != before.revision {
            return Err(Error::CredentialChanged);
        }
        let intent = match journal::begin(&store, uid, now, force, before.rotation.clone())? {
            Begin::Started(intent) => intent,
            Begin::NotDue => return Ok(RefreshOutcome::NotDue),
            Begin::RecoveryRequired => return Ok(RefreshOutcome::RecoveryRequired),
        };
        let client = self.client.clone();
        const PATH: &str = "/api/login/token/refresh/v1";
        let result = self
            .session
            .run_for(&token, move |_, _| async move {
                validate_response(
                    PATH,
                    client
                        .request_jsk_if_revision(PATH, payload, &before.revision)
                        .await?,
                    &[200],
                )
            })
            .await;
        if let Err(error) = result {
            if matches!(error.root(), Error::RequestNotSent(_)) {
                journal::not_sent(&store, uid, &intent)?;
            }
            return Err(error);
        }
        self.finish_jsk(&token, &intent).await
    }

    async fn finish_jsk(&self, token: &SessionToken, intent: &Intent) -> Result<RefreshOutcome> {
        let credentials = self.client.credentials();
        let after = credentials.login_credentials()?;
        if after.token.as_deref().is_none_or(str::is_empty)
            || after.refresh_token.as_deref().is_none_or(str::is_empty)
            || after.rotation.is_none()
            || after.rotation == intent.before_rotation
        {
            return Ok(RefreshOutcome::ReauthenticationRequired);
        }
        self.identity(token).await?;
        self.session.require_api(token, &self.api())?;
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
        let _guard = self.gate.lock().await;
        let token = self.current()?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing authenticated UID"))?;
        let Some(intent) =
            journal::load(&self.client.credentials().store(), uid)?.filter(|i| i.pending)
        else {
            return Ok(RefreshOutcome::NoPendingRefresh);
        };
        self.finish_jsk(&token, &intent).await
    }
}
