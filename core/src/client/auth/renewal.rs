use super::{MusicClient, SessionToken};
use crate::{
    Error, Result,
    account::{
        SessionStatus,
        refresh::{AccountLifecycle, DeviceInfo, DeviceTokenProvider, RefreshOutcome},
    },
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenewalAction {
    Check,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenewalState {
    Unavailable,
    SignedOut,
    Expired,
    LoginInProgress,
    PolicyDisabled,
    Ready,
    Checking,
    MissingCredentials,
    NotDue,
    Refreshed,
    RecoveryRequired,
    ReauthenticationRequired,
    Failed,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRenewalView {
    pub epoch: String,
    pub state: RenewalState,
    /// Unix seconds; neither credential revision nor rotation receipt is public.
    pub last_success: Option<u64>,
    pub busy: bool,
    pub can_check: bool,
    pub can_reconcile: bool,
}

struct NativeRenewal {
    device: DeviceInfo,
    provider: Arc<dyn DeviceTokenProvider>,
}

#[derive(Default)]
pub(super) struct RenewalHost {
    native: Option<NativeRenewal>,
    gate: tokio::sync::Mutex<()>,
    last: Mutex<Option<(SessionToken, RenewalState)>>,
    flight: Mutex<Option<SessionToken>>,
}
struct Flight<'a>(&'a Mutex<Option<SessionToken>>);
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.lock() {
            *active = None;
        }
    }
}

impl MusicClient {
    /// Native opt-in: use the recovered PC experiment assignments for login
    /// interception and foreground renewal. There is no frontend override.
    pub fn with_official_login_policy(self) -> Self {
        self.session.enable_official_login_policy();
        self
    }

    pub fn subscribe_account(&self) -> Result<tokio::sync::watch::Receiver<SessionToken>> {
        self.session.subscribe()
    }

    pub async fn refresh_login_experiments(&self, epoch: &str) -> Result<()> {
        let token = self.auth_token(epoch)?;
        if self.session.snapshot()?.status == SessionStatus::Authenticated
            && !self.login_pending()?
        {
            self.session.refresh_login_policy(&token).await?;
        }
        Ok(())
    }
    /// Native setup only. Device ID always comes from the selected profile,
    /// never from frontend input or a previous account's device template.
    pub fn with_session_renewal(
        mut self,
        mut device: DeviceInfo,
        provider: Arc<dyn DeviceTokenProvider>,
    ) -> Self {
        device.device_id.clear();
        self.auth.renewal.native = Some(NativeRenewal { device, provider });
        self
    }

    pub fn session_renewal(&self, epoch: &str) -> Result<SessionRenewalView> {
        let token = self.auth_token(epoch)?;
        let host = &self.auth.renewal;
        let snapshot = self.session.snapshot()?;
        let busy = host
            .flight
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            == Some(&token);
        let mut last_success = None;
        let state = match snapshot.status {
            SessionStatus::Expired => RenewalState::Expired,
            SessionStatus::Unknown | SessionStatus::SignedOut => RenewalState::SignedOut,
            SessionStatus::Authenticated if self.login_pending()? => RenewalState::LoginInProgress,
            SessionStatus::Authenticated => {
                if let Some(client) = self
                    .native_account_client()?
                    .filter(|_| host.native.is_some())
                {
                    let status = AccountLifecycle::new(self.session.clone(), client.clone())
                        .refresh_status()?;
                    last_success = status.last_success;
                    let credentials = client.credentials().login_credentials()?;
                    let last = host
                        .last
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?
                        .as_ref()
                        .filter(|(t, _)| t == &token)
                        .map(|(_, state)| *state);
                    if busy {
                        RenewalState::Checking
                    } else if status.pending {
                        if last == Some(RenewalState::ReauthenticationRequired) {
                            RenewalState::ReauthenticationRequired
                        } else {
                            RenewalState::RecoveryRequired
                        }
                    } else if credentials.token.as_deref().is_none_or(str::is_empty)
                        || credentials
                            .refresh_token
                            .as_deref()
                            .is_none_or(str::is_empty)
                    {
                        RenewalState::MissingCredentials
                    } else if self.session.login_policy_enabled(&token)? == Some(false) {
                        RenewalState::PolicyDisabled
                    } else {
                        last.unwrap_or(RenewalState::Ready)
                    }
                } else {
                    RenewalState::Unavailable
                }
            }
        };
        self.session.check_generation(&token)?;
        Ok(SessionRenewalView {
            epoch: epoch.into(),
            state,
            last_success,
            busy,
            can_check: !busy
                && matches!(
                    state,
                    RenewalState::Ready
                        | RenewalState::Failed
                        | RenewalState::NotDue
                        | RenewalState::Refreshed
                ),
            can_reconcile: !busy
                && matches!(
                    state,
                    RenewalState::RecoveryRequired | RenewalState::ReauthenticationRequired
                ),
        })
    }

    /// Foreground/manual checks share a single flight and durable daily gate.
    /// Pending writes only reconcile; there is no IPC 'force' or fallback API.
    pub async fn renew_session(
        &self,
        epoch: &str,
        action: RenewalAction,
    ) -> Result<SessionRenewalView> {
        let token = self.auth_token(epoch)?;
        let host = &self.auth.renewal;
        let _gate = host.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        if host.native.is_some() {
            self.refresh_login_experiments(epoch).await?;
        }
        let before = self.session_renewal(epoch)?;
        if !before.can_check && !before.can_reconcile {
            return Ok(before);
        }
        let native = host
            .native
            .as_ref()
            .ok_or(Error::State("session renewal unavailable"))?;
        let client = self
            .native_account_client()?
            .ok_or(Error::State("session renewal unavailable"))?;
        let mut device = native.device.clone();
        device.device_id = client.credentials().device_id()?;
        let lifecycle = AccountLifecycle::new(self.session.clone(), client);
        self.session.check_generation(&token)?;
        *host.flight.lock().map_err(|_| Error::LockPoisoned)? = Some(token.clone());
        let flight = Flight(&host.flight);
        let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            if action == RenewalAction::Reconcile || before.can_reconcile {
                lifecycle.reconcile_jsk().await
            } else {
                lifecycle
                    .refresh_jsk(&device, native.provider.as_ref(), false)
                    .await
            }
        })
        .await
        .unwrap_or_else(|_| Err(Error::Transport("session renewal timeout".into())));
        // An old account's completion must not change the new account's view.
        self.session.check_generation(&token)?;
        let state = match &result {
            Ok(RefreshOutcome::MissingCredentials) => RenewalState::MissingCredentials,
            Ok(RefreshOutcome::NotDue) => RenewalState::NotDue,
            Ok(RefreshOutcome::Refreshed) => RenewalState::Refreshed,
            Ok(RefreshOutcome::RecoveryRequired) => RenewalState::RecoveryRequired,
            Ok(RefreshOutcome::ReauthenticationRequired) => RenewalState::ReauthenticationRequired,
            Ok(RefreshOutcome::NoPendingRefresh) => RenewalState::Ready,
            Err(_) => RenewalState::Failed,
        };
        *host.last.lock().map_err(|_| Error::LockPoisoned)? = Some((token, state));
        drop(flight);
        result?;
        self.session_renewal(epoch)
    }
}
