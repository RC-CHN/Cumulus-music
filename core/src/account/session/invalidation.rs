//! Local invalidation, including recovery across credential/queue stores.
use super::{AccountSession, SessionToken};
use crate::{
    Error, Result, account::login_policy::LoginNotice, playback::QueueResetPlan,
    protocol::http::MusicHttpClient,
};

impl AccountSession {
    // Caller holds SessionState and has checked the request generation. No
    // await or network operation may occur between that check and these writes.
    pub(super) fn invalidate_native_login(
        &self,
        client: &MusicHttpClient,
        token: &SessionToken,
        notice: &LoginNotice,
        clear_queue: bool,
    ) -> Result<()> {
        let plan = if clear_queue {
            let store = self
                .store
                .as_ref()
                .ok_or(Error::State("queue store unavailable"))?;
            Some(QueueResetPlan::capture(
                store,
                token.uid.as_deref().unwrap_or("local-guest"),
            )?)
        } else {
            None
        };
        client.credentials().invalidate_login(notice, plan)?;
        self.recover_queue_reset(client)
    }
    pub(super) fn recover_queue_reset(&self, client: &MusicHttpClient) -> Result<()> {
        let credentials = client.credentials();
        if let Some(plan) = credentials.pending_queue_reset()? {
            let store = self
                .store
                .as_ref()
                .ok_or(Error::State("queue store unavailable"))?;
            plan.apply(store)?;
            credentials.finish_queue_reset(plan.id())?;
        }
        Ok(())
    }
}
