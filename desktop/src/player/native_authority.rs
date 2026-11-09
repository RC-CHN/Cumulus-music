//! A permit belongs to a particular renderer subscription, not just an account.
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const RESERVED: u8 = 0;
const READY: u8 = 1;
const REVOKED: u8 = 2;

#[derive(Clone, Default)]
pub(super) struct Lease(Arc<AtomicU8>);

impl Lease {
    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn ready(&self) -> bool {
        self.0.load(Ordering::Acquire) == READY
    }

    #[cfg(windows)]
    pub(super) fn active(&self) -> bool {
        self.0.load(Ordering::Acquire) != REVOKED
    }

    pub(super) fn install(&self) -> bool {
        self.0
            .compare_exchange(RESERVED, READY, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn revoke(&self) {
        self.0.store(REVOKED, Ordering::Release);
    }
}

/// Keep this in the subscription future from creation, including before its
/// first poll. Dropping an uninstalled registration also revokes its permit.
pub(super) struct RevokeOnDrop(Option<Lease>);

impl RevokeOnDrop {
    pub(super) fn new(lease: &Lease) -> Self {
        Self(Some(lease.clone()))
    }

    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for RevokeOnDrop {
    fn drop(&mut self) {
        if let Some(lease) = self.0.take() {
            lease.revoke();
        }
    }
}
