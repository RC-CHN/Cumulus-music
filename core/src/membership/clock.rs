use crate::{Error, Result};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub struct ClockReading {
    pub wall_seconds: f64,
    pub monotonic_seconds: f64,
}
impl ClockReading {
    pub(crate) fn validate(self) -> Result<Self> {
        if !self.wall_seconds.is_finite()
            || !self.monotonic_seconds.is_finite()
            || self.monotonic_seconds < 0.0
        {
            return Err(Error::State("invalid membership clock"));
        }
        Ok(self)
    }
}
pub trait MembershipClock: Send + Sync {
    fn now(&self) -> Result<ClockReading>;
}
pub struct SystemMembershipClock {
    epoch: Instant,
}
impl Default for SystemMembershipClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}
impl MembershipClock for SystemMembershipClock {
    fn now(&self) -> Result<ClockReading> {
        Ok(ClockReading {
            wall_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("system clock precedes UNIX epoch"))?
                .as_secs_f64(),
            monotonic_seconds: self.epoch.elapsed().as_secs_f64(),
        })
    }
}
