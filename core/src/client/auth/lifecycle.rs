use super::*;
use crate::account::refresh::AccountLifecycle;

impl MusicClient {
    pub async fn remove_saved_account(&self, epoch: &str, id: &str) -> Result<()> {
        let _gate = self.together.gate.lock().await;
        let token = self.auth_token(epoch)?;
        self.registry.remove_saved(&self.session, &token, id)
    }

    pub async fn logout_account(&self, epoch: &str, remote: bool) -> Result<LogoutView> {
        let _gate = self.together.gate.lock().await;
        self.auth_token(epoch)?;
        self.check_account_transition()?;
        {
            let mut slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            if let Some(pending) = slot.as_ref() {
                pending.cancel()?;
            }
            *slot = None;
        }
        let client = self
            .native_account_client()?
            .ok_or(Error::State("native account lifecycle is unavailable"))?;
        let result = AccountLifecycle::new(self.session.clone(), client)
            .logout(remote)
            .await?;
        self.suspend_account_media().await?;
        Ok(LogoutView {
            account: self.account()?,
            remote_revoked: result.remote_revoked,
        })
    }
}
