//! The M/623 long-audio chart decision comes only from native PC assignment.
use crate::{
    Result,
    account::{AccountSession, SessionToken},
    protocol::api::Api,
};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

const REFRESH: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(15);
pub(crate) const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub(super) struct AudioToplist(Mutex<Option<Assignment>>);

struct Assignment {
    owner: SessionToken,
    api: Weak<dyn Api>,
    assigned: bool,
    expires: Instant,
}

impl AudioToplist {
    pub(super) async fn assigned(
        &self,
        session: &AccountSession,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<bool> {
        let mut cached = self.0.lock().await;
        session.require_api(owner, api)?;
        let previous = cached.as_ref().filter(|entry| {
            entry.owner == *owner
                && entry
                    .api
                    .upgrade()
                    .is_some_and(|old| Arc::ptr_eq(&old, api))
        });
        if let Some(entry) = previous.filter(|entry| entry.expires > Instant::now()) {
            return Ok(entry.assigned);
        }
        let (assigned, ttl) =
            match tokio::time::timeout(TIMEOUT, api.search_audio_toplist_assignment()).await {
                Ok(Ok(assigned)) => (assigned, REFRESH),
                // A failed refresh cannot promote a new owner. Keep the same
                // already verified group as the official assignment cache does.
                _ => (previous.is_some_and(|entry| entry.assigned), RETRY),
            };
        session.require_api(owner, api)?;
        *cached = Some(Assignment {
            owner: owner.clone(),
            api: Arc::downgrade(api),
            assigned,
            expires: Instant::now() + ttl,
        });
        Ok(assigned)
    }
}
