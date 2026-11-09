//! Windows native file requests must not survive a confirmed quit attempt.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct Gate(Arc<AtomicU64>);

#[derive(Clone)]
pub(super) struct Permit {
    state: Arc<AtomicU64>,
    generation: u64,
}

impl Permit {
    pub(super) fn active(&self) -> bool {
        self.state.load(Ordering::Acquire) == self.generation
    }
}

/// Only a failure in this exact quit attempt can reopen native activation.
pub(crate) struct Recovery {
    state: Arc<AtomicU64>,
    generation: u64,
}

impl Recovery {
    pub(crate) fn reopen(self) -> bool {
        self.generation.checked_add(1).is_some_and(|open| {
            self.state
                .compare_exchange(self.generation, open, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        })
    }
}

impl Gate {
    pub(super) fn permit(&self) -> Option<Permit> {
        let generation = self.0.load(Ordering::Acquire);
        (generation & 1 == 0).then(|| Permit {
            state: self.0.clone(),
            generation,
        })
    }

    pub(super) fn close(&self) -> Option<Recovery> {
        // Even is open, odd is closed. A newer quit while already closed
        // advances to a new odd value instead of reviving any old permit.
        let previous = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1).map(|next| next | 1)
            })
            .ok()?;
        Some(Recovery {
            state: self.0.clone(),
            generation: (previous + 1) | 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_admits_only_new_requests_and_only_for_the_latest_failed_quit() {
        let gate = Gate::default();
        let prior = gate.permit().unwrap();
        assert!(prior.active());
        let first = gate.close().unwrap();
        assert!(gate.permit().is_none());
        assert!(!prior.active());
        let latest = gate.close().unwrap();
        assert!(!first.reopen());
        assert!(gate.permit().is_none());
        assert!(latest.reopen());
        assert!(!prior.active());
        let new = gate.permit().unwrap();
        assert!(new.active());
        // Returning from a successful quit cannot reopen the gate.
        let completed_quit = gate.close().unwrap();
        let _leave_closed = completed_quit;
        assert!(!new.active());
        assert!(gate.permit().is_none());
    }
}
