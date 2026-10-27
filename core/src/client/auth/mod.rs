mod captcha;
mod dto;
mod flow;
mod lifecycle;
mod qr;
mod renewal;
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
pub use renewal::{RenewalAction, RenewalState, SessionRenewalView};
use std::sync::{Arc, Mutex};

enum PendingFlow {
    Qr(Arc<QrAuthentication>),
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
            PendingFlow::Credentials { flow, .. } => flow.cancel(),
        }
    }
}

#[derive(Default)]
pub(super) struct AuthHost {
    pub(super) profile: ClientProfile,
    pub(super) context: Option<Arc<crate::protocol::context::RequestContext>>,
    pub(super) options: RequestOptions,
    pending: Mutex<Option<PendingLogin>>,
    backend: Option<Arc<dyn HttpBackend>>,
    guardian: Option<Arc<dyn LoginGuardian>>,
    cooldown: Arc<ResendCooldown>,
    renewal: renewal::RenewalHost,
}
impl AuthHost {
    fn replace(&self, pending: PendingLogin) -> Result<()> {
        let mut slot = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(previous) = slot.as_ref() {
            previous.cancel()?;
        }
        *slot = Some(pending);
        Ok(())
    }
}
impl MusicClient {
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
    fn login_candidate(&self, saved: Option<&str>) -> Result<LoginCandidate> {
        match (&self.auth.backend, saved) {
            (Some(backend), Some(id)) => self.registry.saved_candidate_with_backend(
                id,
                self.auth.profile.clone(),
                self.auth.options,
                self.auth.context.clone(),
                backend.clone(),
            ),
            (Some(backend), None) => self.registry.candidate_with_backend(
                self.auth.profile.clone(),
                self.auth.options,
                self.auth.context.clone(),
                backend.clone(),
            ),
            (None, saved) => {
                let backend = Arc::new(crate::protocol::http::ReqwestBackend::new(
                    std::time::Duration::from_secs(20),
                )?);
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
        }
    }
    fn auth_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        Ok(token)
    }
    pub(super) fn login_pending(&self) -> Result<bool> {
        self.auth
            .pending
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .map_or(Ok(false), |p| p.authenticated().map(|done| !done))
    }
    pub fn cancel_login(&self, id: &str) -> Result<()> {
        let mut slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
        let pending = slot
            .as_ref()
            .filter(|p| p.id == id)
            .ok_or(Error::StaleResponse)?;
        pending.cancel()?;
        *slot = None;
        Ok(())
    }
}
