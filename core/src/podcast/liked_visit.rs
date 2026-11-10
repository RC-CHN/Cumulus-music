//! Active liked-page revision held across its synchronous queue admission.
use crate::{
    Error, Result, account::SessionToken, podcast::cache_generation_in, storage::StateTransaction,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct LikedVisit(Arc<Mutex<bool>>);

impl Default for LikedVisit {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(true)))
    }
}

impl LikedVisit {
    pub(crate) fn active(&self) -> bool {
        self.0.lock().is_ok_and(|active| *active)
    }

    pub(crate) fn retire(&self) {
        // Retiring is one way, including on a panic while an edit held it.
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = false;
    }

    pub(crate) fn fence(&self, token: &SessionToken, generation: u64) -> LikedPageQueueFence {
        LikedPageQueueFence {
            visit: self.clone(),
            token: token.clone(),
            generation,
        }
    }
}

/// A native receipt for a specific active revision and its observed durable
/// podcast generation. Only the liked-page host can issue it.
#[derive(Clone)]
pub struct LikedPageQueueFence {
    visit: LikedVisit,
    token: SessionToken,
    generation: u64,
}

impl LikedPageQueueFence {
    /// Caller holds AccountSession first. Keep this pure lock through SQLite
    /// commit; the callback must not re-enter AccountSession or the visit.
    pub(crate) fn with_active<T>(
        &self,
        token: &SessionToken,
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let active = self.visit.0.lock().map_err(|_| Error::LockPoisoned)?;
        if &self.token != token || !*active {
            return Err(Error::QueueChanged);
        }
        let result = commit();
        drop(active);
        result
    }

    pub(crate) fn verify_generation(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        if self.token.uid.as_deref() != Some(owner)
            || cache_generation_in(tx, owner)? != self.generation
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}
