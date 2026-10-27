use super::ResolvedSource;
use crate::{Error, Result, account::SessionToken};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub token: SessionToken,
    pub id: String,
    pub voice_id: Option<String>,
    pub level: String,
    pub encode: String,
    pub trial: i32,
}
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct PendingKey {
    pub revision: u64,
    pub key: Key,
}
pub(super) struct Cached {
    pub value: Arc<ResolvedSource>,
    pub deadline: Duration,
}
pub(super) type Outcome = std::result::Result<Arc<ResolvedSource>, Arc<Error>>;
#[derive(Default)]
pub(super) struct CacheState {
    pub revision: u64,
    pub cache: HashMap<Key, Cached>,
    pub pending: HashMap<PendingKey, watch::Sender<Option<Outcome>>>,
}

/// Dropping the network Future wakes subscribers and removes the pending job.
/// It cannot leave a poisoned per-key gate or silently replay a failed request.
pub(super) struct Owner {
    state: Arc<Mutex<CacheState>>,
    key: Option<PendingKey>,
}
impl Owner {
    pub fn new(state: Arc<Mutex<CacheState>>, key: PendingKey) -> Self {
        Self {
            state,
            key: Some(key),
        }
    }
    pub fn finish(mut self, mut value: Outcome) -> Result<Outcome> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if self
            .key
            .as_ref()
            .is_some_and(|key| key.revision != state.revision)
        {
            value = Err(Arc::new(Error::StaleResponse));
        }
        if let Some(key) = self.key.take()
            && let Some(sender) = state.pending.remove(&key)
        {
            sender.send_replace(Some(value.clone()));
        }
        Ok(value)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(key) = self.key.take()
            && let Ok(mut state) = self.state.lock()
            && let Some(sender) = state.pending.remove(&key)
        {
            sender.send_replace(Some(Err(Arc::new(Error::Cancelled))));
        }
    }
}
