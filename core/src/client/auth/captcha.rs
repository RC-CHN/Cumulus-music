//! User-completed official CAPTCHA. Native provider, no frontend proof input.
use super::*;
use crate::{account::QrStatus, protocol::api::ApiFuture};
use tokio::sync::oneshot;

pub trait LoginCaptchaProvider: Send + Sync {
    /// Dropping this future must close its UI and discard the pending proof.
    fn solve(&self) -> ApiFuture<'_, String>;
}

#[derive(Clone, Default)]
pub(super) struct Flight(Arc<Mutex<Option<oneshot::Sender<()>>>>);
impl Flight {
    pub fn active(&self) -> Result<bool> {
        Ok(self.0.lock().map_err(|_| Error::LockPoisoned)?.is_some())
    }
    pub fn check_idle(&self) -> Result<()> {
        if self.active()? {
            return Err(Error::State("CAPTCHA is already pending"));
        }
        Ok(())
    }
    pub fn cancel(&self) -> Result<()> {
        if let Some(sender) = self.0.lock().map_err(|_| Error::LockPoisoned)?.take() {
            let _ = sender.send(());
        }
        Ok(())
    }
    fn begin(&self) -> Result<(Guard, oneshot::Receiver<()>)> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.is_some() {
            return Err(Error::State("CAPTCHA is already pending"));
        }
        let (sender, receiver) = oneshot::channel();
        *slot = Some(sender);
        Ok((Guard(self.clone()), receiver))
    }
}
struct Guard(Flight);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.cancel();
    }
}

impl MusicClient {
    pub async fn solve_login_captcha(
        &self,
        id: &str,
        provider: &dyn LoginCaptchaProvider,
    ) -> Result<LoginCaptchaView> {
        let (guard, mut cancelled, base) = {
            let _gate = self.together.gate.lock().await;
            self.check_account_transition()?;
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let pending = slot
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            match &pending.flow {
                PendingFlow::Credentials { flow, .. } if flow.snapshot()?.captcha_available => {}
                PendingFlow::Qr(flow) if flow.snapshot()?.status == QrStatus::Challenge => (),
                _ => return Err(Error::State("no CAPTCHA challenge is pending")),
            }
            let (guard, cancelled) = pending.captcha.begin()?;
            (guard, cancelled, self.session.token()?)
        };
        // Do not hold the account transition gate while waiting for a person.
        let proof = tokio::select! {
            biased;
            _ = &mut cancelled => return Err(Error::Cancelled),
            result = tokio::time::timeout(std::time::Duration::from_secs(60), provider.solve()) => {
                result.map_err(|_| Error::State("CAPTCHA timed out"))??
            }
        };
        if proof.is_empty() || proof.len() > 8192 || proof.chars().any(char::is_control) {
            return Err(Error::InvalidInput("invalid completed CAPTCHA result"));
        }
        let _gate = self.together.gate.lock().await;
        self.session.check_generation(&base)?;
        self.check_account_transition()?;
        let (credentials, qr) = {
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let pending = slot
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            match &pending.flow {
                PendingFlow::Credentials { flow, .. } => (Some(flow.clone()), None),
                PendingFlow::Qr(flow) => (None, Some(flow.clone())),
            }
        };
        if let Some(flow) = credentials {
            flow.submit_captcha(&proof).await?;
            drop(guard);
            let view = self.login_snapshot(id)?;
            if view.status == LoginStatus::Authenticated {
                self.suspend_account_media().await?;
            }
            Ok(LoginCaptchaView::Credentials(Box::new(view)))
        } else {
            let status = qr.ok_or(Error::StaleResponse)?.poll(Some(&proof)).await?;
            drop(guard);
            self.qr_flow(id)?;
            if status == QrStatus::Authorized {
                self.suspend_account_media().await?;
            }
            Ok(LoginCaptchaView::Qr(status))
        }
    }
}
