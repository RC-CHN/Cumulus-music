mod captcha;
mod dto;
mod flow;
mod lifecycle;
mod presentation;
mod qr;
mod renewal;
mod server_quick;
mod view;
use super::{MusicClient, mapping};
use crate::{
    Error, Result,
    account::{
        SessionToken,
        login::{CredentialLogin, LoginGuardian, LoginStatus, QrAuthentication, ResendCooldown},
        profiles::LoginCandidate,
    },
    protocol::http::{ClientProfile, HttpBackend, RequestOptions},
};
pub use captcha::LoginCaptchaProvider;
pub use dto::*;
pub use presentation::LoginPresentation;
pub use renewal::{RenewalAction, RenewalState, SessionRenewalView};
pub use server_quick::{ServerQuickLoginCommand, ServerQuickLoginOperationView};
use std::sync::{Arc, Mutex};

enum PendingFlow {
    Qr(Arc<QrAuthentication>),
    ServerQuick(Arc<crate::account::profiles::ServerQuickLogin>),
    Credentials {
        mode: LoginMode,
        flow: Arc<CredentialLogin>,
    },
}
struct PendingLogin {
    id: String,
    flow: PendingFlow,
    view: LoginChallengeView,
    captcha: captcha::Flight,
}
impl PendingLogin {
    fn authenticated(&self) -> Result<bool> {
        match &self.flow {
            PendingFlow::Qr(flow) => flow.authenticated(),
            PendingFlow::ServerQuick(flow) => Ok(flow.snapshot()?.phase
                == crate::account::profiles::ServerQuickLoginPhase::Authenticated),
            PendingFlow::Credentials { flow, .. } => {
                Ok(flow.snapshot()?.status == LoginStatus::Authenticated)
            }
        }
    }
    fn cancel(&self) -> Result<()> {
        self.captcha.cancel()?;
        if self.authenticated()? {
            return Ok(());
        }
        match &self.flow {
            PendingFlow::Qr(flow) => flow.cancel(),
            PendingFlow::ServerQuick(flow) => flow.cancel(),
            PendingFlow::Credentials { flow, .. } => flow.cancel(),
        }
    }
}

#[derive(Default)]
pub(super) struct AuthHost {
    pub(super) profile: ClientProfile,
    pub(super) context: Option<Arc<crate::protocol::context::RequestContext>>,
    pub(super) options: RequestOptions,
    pending: Arc<Mutex<Option<PendingLogin>>>,
    pub(super) backend: Option<Arc<dyn HttpBackend>>,
    guardian: Option<Arc<dyn LoginGuardian>>,
    device_name: Option<String>,
    cooldown: Arc<ResendCooldown>,
    renewal: renewal::RenewalHost,
}
impl AuthHost {
    pub(super) fn pending_guard(&self) -> impl Fn() -> Result<()> + Send + Sync + 'static + use<> {
        let pending = self.pending.clone();
        move || {
            if login_pending(&pending)? {
                return Err(Error::StaleResponse);
            }
            Ok(())
        }
    }

    fn replace(&self, pending: PendingLogin) -> Result<()> {
        let mut slot = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(previous) = slot.as_ref() {
            previous.cancel()?;
        }
        *slot = Some(pending);
        Ok(())
    }
}

fn login_pending(pending: &Mutex<Option<PendingLogin>>) -> Result<bool> {
    pending
        .lock()
        .map_err(|_| Error::LockPoisoned)?
        .as_ref()
        .map_or(Ok(false), |p| p.authenticated().map(|done| !done))
}

impl MusicClient {
    /// Native opt-in; no renderer command and no production startup until
    /// the host has verified this lane independently of JSK renewal.
    pub fn start_legacy_session_renewal(&self) -> Result<crate::account::refresh::LegacyRenewal> {
        crate::account::refresh::LegacyRenewal::start(self.session.clone())
    }

    /// Native host clock injection before exposing the client. No renderer
    /// command can reset or advance verification-code cooldowns.
    pub fn with_login_resend_clock(
        mut self,
        clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>,
    ) -> Self {
        self.auth.cooldown = Arc::new(ResendCooldown::with_clock(clock));
        self
    }
    /// Official original-device verification uses the local computer name.
    /// This native configuration is distinct from the renewal signature model.
    pub fn with_login_device_name(mut self, name: &str) -> Result<Self> {
        if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
            return Err(Error::InvalidInput("invalid native login device name"));
        }
        self.auth.device_name = Some(name.into());
        Ok(self)
    }
    fn check_account_transition(&self) -> Result<()> {
        let token = self.session.token()?;
        if token.uid.is_none() {
            return Ok(());
        }
        let service = crate::listen_together::TogetherService::new(self.session.clone())?;
        if self.has_together_cleanup()?
            || service.state_for(&token)?.room.is_some()
            || service
                .operations_for(&token)?
                .iter()
                .any(|op| op.phase.pending())
        {
            return Err(Error::State("leave together before switching accounts"));
        }
        Ok(())
    }
    /// Configure native transport/guardian providers before exposing this client
    /// to a host. The frontend cannot inject endpoints, backends or SDK tokens.
    pub fn with_login_providers(
        mut self,
        backend: Option<Arc<dyn HttpBackend>>,
        guardian: Option<Arc<dyn LoginGuardian>>,
    ) -> Self {
        self.auth.backend = backend;
        self.auth.guardian = guardian;
        self
    }
    fn candidate_backend(&self) -> Result<Arc<dyn HttpBackend>> {
        match &self.auth.backend {
            Some(backend) => Ok(backend.clone()),
            None => Ok(Arc::new(crate::protocol::http::ReqwestBackend::new(
                std::time::Duration::from_secs(20),
            )?)),
        }
    }

    fn login_candidate(&self, saved: Option<&str>) -> Result<LoginCandidate> {
        let backend = self.candidate_backend()?;
        match saved {
            Some(id) => self.registry.saved_candidate_with_backend(
                id,
                self.auth.profile.clone(),
                self.auth.options,
                self.auth.context.clone(),
                backend,
            ),
            None => self.registry.candidate_with_backend(
                self.auth.profile.clone(),
                self.auth.options,
                self.auth.context.clone(),
                backend,
            ),
        }
    }

    fn qr_login_candidate(&self) -> Result<LoginCandidate> {
        self.registry.fresh_qr_candidate(
            self.auth.profile.clone(),
            self.auth.options,
            self.auth.context.clone(),
            self.candidate_backend()?,
        )
    }
    fn auth_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        Ok(token)
    }
    pub(super) fn login_pending(&self) -> Result<bool> {
        login_pending(&self.auth.pending)
    }
    pub fn cancel_login(&self, id: &str) -> Result<()> {
        self.cancel_login_parts(id, false).map(|_| ())
    }
    /// Revoke locally first, then perform the legacy PC close cleanup once.
    /// A lost cleanup response is returned, never automatically repeated.
    pub async fn cancel_login_and_cleanup(&self, id: &str) -> Result<()> {
        if let Some(cleanup) = self.cancel_login_parts(id, true)? {
            cleanup.send().await?;
        }
        Ok(())
    }
    fn cancel_login_parts(
        &self,
        id: &str,
        remote: bool,
    ) -> Result<Option<crate::account::login::DeviceCancellation>> {
        let mut slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
        let pending = slot
            .as_ref()
            .filter(|p| p.id == id)
            .ok_or(Error::StaleResponse)?;
        let cleanup = if remote {
            match &pending.flow {
                PendingFlow::Credentials { flow, .. } => flow.take_device_cleanup()?,
                PendingFlow::Qr(_) | PendingFlow::ServerQuick(_) => None,
            }
        } else {
            None
        };
        pending.cancel()?;
        *slot = None;
        Ok(cleanup)
    }
}
