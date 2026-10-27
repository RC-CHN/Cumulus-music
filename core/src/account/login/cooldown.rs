use crate::{Error, Result};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Share across attempts so a new candidate cannot reset an SMS/email cooldown.
pub struct ResendCooldown {
    deadline: Mutex<Option<Instant>>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for ResendCooldown {
    fn default() -> Self {
        Self::with_clock(Arc::new(Instant::now))
    }
}

impl ResendCooldown {
    pub fn with_clock(clock: Arc<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self {
            deadline: Mutex::new(None),
            clock,
        }
    }
    pub fn remaining(&self) -> Result<Duration> {
        Ok(self
            .deadline
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .map_or(Duration::ZERO, |end| {
                end.saturating_duration_since((self.clock)())
            }))
    }
    pub(super) fn reserve(&self) -> Result<()> {
        let now = (self.clock)();
        let mut deadline = self.deadline.lock().map_err(|_| Error::LockPoisoned)?;
        if deadline.is_some_and(|end| end > now) {
            return Err(Error::State("verification resend cooldown is active"));
        }
        *deadline = Some(now + Duration::from_secs(60));
        Ok(())
    }
}
