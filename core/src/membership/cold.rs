//! Previously received membership evidence, without an account session or API.
use super::{
    OfflineSource, Quality,
    clock::{ClockReading, MembershipClock, SystemMembershipClock},
    manager::Record,
    membership_state, offline_access,
};
use crate::{Error, Result, storage::StateStore};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(crate) struct ColdMembership {
    clock: Arc<dyn MembershipClock>,
    anchored: Mutex<Option<Arc<Anchor>>>,
}
impl Default for ColdMembership {
    fn default() -> Self {
        Self::with_clock(Arc::new(SystemMembershipClock::default()))
    }
}
impl ColdMembership {
    pub(crate) fn with_clock(clock: Arc<dyn MembershipClock>) -> Self {
        Self {
            clock,
            anchored: Mutex::new(None),
        }
    }
}
struct Anchor {
    raw: Value,
    record: Record,
    at: ClockReading,
    invalid: AtomicBool,
    elapsed_high_water: AtomicU64,
}
impl Anchor {
    fn now(&self, clock: &dyn MembershipClock) -> Result<f64> {
        let now = clock.now()?.validate()?;
        let monotonic = now.monotonic_seconds - self.at.monotonic_seconds;
        let wall = now.wall_seconds - self.at.wall_seconds;
        // A monotonic clock can stop during system suspend. Wall time can
        // advance the deadline, but neither clock can grant back elapsed time.
        let elapsed = monotonic.max(wall).max(0.0);
        if !elapsed.is_finite() || monotonic < 0.0 {
            self.invalid.store(true, Ordering::Release);
        } else {
            // Nonnegative finite floats order the same as their unsigned bits.
            let prior = self
                .elapsed_high_water
                .fetch_max(elapsed.to_bits(), Ordering::AcqRel);
            if wall < f64::from_bits(prior).max(monotonic) - 5.0 {
                self.invalid.store(true, Ordering::Release);
            }
        }
        if self.invalid.load(Ordering::Acquire) {
            return Err(Error::Response(
                "clock moved backwards; refresh membership online",
            ));
        }
        Ok(self.record.server_ms
            + f64::from_bits(self.elapsed_high_water.load(Ordering::Acquire)) * 1000.0)
    }
}

/// Playback-time predicate. Callback threads read only clocks and atomics;
/// the host's ordinary blocking checks verify the persisted cache revision.
#[derive(Clone)]
pub(crate) struct ColdEntitlement {
    anchor: Arc<Anchor>,
    clock: Arc<dyn MembershipClock>,
    deadline: Option<f64>,
}
impl ColdEntitlement {
    pub(crate) fn valid(&self) -> bool {
        self.anchor
            .now(&*self.clock)
            .is_ok_and(|now| now.is_finite() && self.deadline.is_none_or(|deadline| now < deadline))
    }

    pub(crate) fn validate(&self, store: &StateStore, owner: &str) -> Result<()> {
        if store.get(owner, "membership", "current")?.as_ref() != Some(&self.anchor.raw) {
            self.anchor.invalid.store(true, Ordering::Release);
        }
        if !self.valid() {
            return Err(Error::State(
                "cached offline entitlement expired or changed",
            ));
        }
        Ok(())
    }
}
impl ColdMembership {
    pub(crate) fn entitlement(
        &self,
        store: &StateStore,
        owner: &str,
        privilege: &Value,
        quality: Quality,
    ) -> Result<ColdEntitlement> {
        let mut slot = self.anchored.lock().map_err(|_| Error::LockPoisoned)?;
        let Some(raw) = store.get(owner, "membership", "current")? else {
            if let Some(previous) = slot.as_ref() {
                previous.invalid.store(true, Ordering::Release);
            }
            return Err(Error::Response(
                "membership has not been refreshed for this account",
            ));
        };
        if !slot.as_ref().is_some_and(|cached| cached.raw == raw) {
            if let Some(previous) = slot.as_ref() {
                previous.invalid.store(true, Ordering::Release);
            }
            let record: Record = serde_json::from_value(raw.clone())?;
            let at = self.clock.now()?.validate()?;
            let record = record.restore(owner, at, 0.0)?;
            *slot = Some(Arc::new(Anchor {
                raw,
                record,
                at,
                invalid: AtomicBool::new(false),
                elapsed_high_water: AtomicU64::new(0),
            }));
        }
        let anchor = slot
            .as_ref()
            .ok_or(Error::State("missing cold membership anchor"))?;
        let now = anchor.now(&*self.clock)?;
        let allows = |at| {
            offline_access(
                Some(privilege),
                quality,
                &membership_state(&anchor.record.data, at)?,
                OfflineSource::Downloaded { complete: true },
                true,
            )
            .map(|access| access.can_play)
        };
        if !allows(now)? {
            return Err(Error::State(
                "cached membership does not authorize this download",
            ));
        }
        // These are the only times that M/224 can change M/23's decision.
        // Evaluate the same policy at each boundary, so an unrelated expiry
        // or a second valid right cannot stop authorized playback early.
        let state = membership_state(&anchor.record.data, now)?;
        let mut expiry = [
            state.vinyl_vip_expire_time,
            state.plus_vip_expire_time,
            state.music_package_expire_time,
            state.album_vip_expire_time,
        ];
        expiry.sort_unstable_by(f64::total_cmp);
        let mut deadline = None;
        for at in expiry.into_iter().filter(|at| *at > now) {
            if !allows(at)? {
                deadline = Some(at);
                break;
            }
        }
        Ok(ColdEntitlement {
            anchor: anchor.clone(),
            clock: self.clock.clone(),
            deadline,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeClock(Mutex<ClockReading>);
    impl MembershipClock for FakeClock {
        fn now(&self) -> Result<ClockReading> {
            Ok(*self.0.lock().unwrap())
        }
    }
    impl FakeClock {
        fn set(&self, wall_seconds: f64, monotonic_seconds: f64) {
            *self.0.lock().unwrap() = ClockReading {
                wall_seconds,
                monotonic_seconds,
            };
        }
    }
    fn membership() -> (Arc<FakeClock>, ColdMembership) {
        let clock = Arc::new(FakeClock(Mutex::new(ClockReading {
            wall_seconds: 1000.0,
            monotonic_seconds: 1.0,
        })));
        let membership = ColdMembership {
            clock: clock.clone(),
            anchored: Mutex::new(None),
        };
        (clock, membership)
    }
    fn saved(uid: &str, wall: f64) -> Value {
        json!({
            "data":{"userId":uid,"now":100000,"associator":{"expireTime":102000},
                    "redplus":{"expireTime":101000},"musicPackage":{"expireTime":103000},
                    "albumVip":{"expireTime":104000}},
            "server_ms":100000,"wall":wall
        })
    }
    #[test]
    fn cached_rights_follow_actual_quality_purchase_album_and_exact_expiry() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        let (clock, rights) = membership();
        store
            .put("1", "membership", "current", &saved("1", 1000.0))
            .unwrap();
        let obtain = |privilege: Value, quality| {
            rights
                .entitlement(&store, "1", &privilege, quality)
                .unwrap()
        };
        let plus = obtain(json!({"fee":1,"payed":2}), Quality::Sky);
        let normal = obtain(json!({"fee":1}), Quality::Standard);
        let album = obtain(json!({"fee":1,"flag":8192,"payed":2}), Quality::Sky);
        let bought = obtain(json!({"fee":1,"payed":2}), Quality::Standard);
        let free = obtain(json!({"fee":0}), Quality::Sky);
        clock.set(1001.0, 2.0);
        assert!(!plus.valid());
        assert!(album.valid());
        clock.set(1002.0, 3.0);
        assert!(normal.valid(), "music package outlasts vinyl");
        clock.set(1003.0, 4.0);
        assert!(!normal.valid());
        assert!(album.valid());
        clock.set(1004.0, 5.0);
        assert!(!album.valid());
        assert!(bought.valid());
        assert!(free.valid());
        store.delete("1", "membership", "current").unwrap();
        assert!(bought.validate(&store, "1").is_err());
        assert!(!free.valid(), "cache invalidation revokes all its rights");
    }

    #[test]
    fn wall_clock_catches_suspend_and_a_smaller_rollback_cannot_reinstate_rights() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        let (clock, rights) = membership();
        store
            .put("1", "membership", "current", &saved("1", 1000.0))
            .unwrap();
        let obtain = || rights.entitlement(&store, "1", &json!({"fee":1}), Quality::Sky);
        let plus = obtain().unwrap();
        clock.set(1002.0, 1.0); // monotonic clock did not advance while suspended
        assert!(!plus.valid());
        clock.set(1000.5, 1.1); // small wall adjustment is allowed, not a time refund
        assert!(!plus.valid());
        assert!(obtain().is_err());
    }

    #[test]
    fn missing_wrong_owner_rollback_and_regressed_monotonic_clock_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        let (clock, rights) = membership();
        let obtain = || rights.entitlement(&store, "1", &json!({"fee":0}), Quality::Standard);
        store
            .put("2", "membership", "current", &saved("2", 1000.0))
            .unwrap();
        assert!(obtain().is_err(), "other owner's row cannot grant access");
        store
            .put("1", "membership", "current", &saved("2", 1000.0))
            .unwrap();
        assert!(obtain().is_err(), "row's claimed owner must match");
        store
            .put("1", "membership", "current", &saved("1", 1000.1))
            .unwrap();
        assert!(obtain().is_err(), "rollback across restart must be denied");
        store
            .put("1", "membership", "current", &saved("1", 1000.0))
            .unwrap();
        let right = obtain().unwrap();
        clock.set(990.0, 2.0);
        assert!(!right.valid());
        clock.set(1001.0, 2.0);
        assert!(!right.valid(), "rollback denial is latched");
        let (clock, rights) = membership();
        let right = rights
            .entitlement(&store, "1", &json!({"fee":0}), Quality::Standard)
            .unwrap();
        clock.set(1000.0, 0.5);
        assert!(!right.valid());
    }
}
