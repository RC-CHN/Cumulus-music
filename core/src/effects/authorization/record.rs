use crate::{
    Error, Result,
    effects::{catalog::EffectId, rights::Authorization},
};
use serde::{Deserialize, Serialize};

/// This tracks the server registration, never whether audio DSP is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeasePhase {
    Registering,
    Registered,
    StartUncertain,
    StartRejected,
    Releasing,
    Released,
    StopUncertain,
    StopRejected,
}
impl LeasePhase {
    pub fn needs_release(self) -> bool {
        !matches!(self, Self::Released | Self::StartRejected)
    }
    pub fn uncertain(self) -> bool {
        matches!(
            self,
            Self::Registering | Self::StartUncertain | Self::Releasing | Self::StopUncertain
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EffectLease {
    pub effect: EffectId,
    pub operation: String,
    pub revision: u64,
    pub phase: LeasePhase,
    pub start_acknowledged: bool,
    pub stop_acknowledged: bool,
    pub authorization: Option<Authorization>,
    pub recyclable: Option<bool>,
    pub last_error: Option<String>,
}
impl EffectLease {
    pub(super) fn validate(&self, effect: EffectId) -> Result<()> {
        if self.effect != effect
            || self.operation.len() != 32
            || !self
                .operation
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || self.revision == 0
        {
            return Err(Error::State(
                "invalid effect authorization journal identity",
            ));
        }
        if self.phase == LeasePhase::Registered && !self.start_acknowledged
            || self.phase == LeasePhase::Released && !self.stop_acknowledged
        {
            return Err(Error::State(
                "effect authorization journal acknowledgement mismatch",
            ));
        }
        Ok(())
    }
    pub(super) fn advance(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::State("effect authorization revision exhausted"))?;
        Ok(())
    }
}

/// No default consent: beginning a 50/51 trial requires an explicit caller
/// action. The service never purchases an entitlement or grants missing rights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrialConsent {
    DoNotStart,
    ExplicitStart,
}

/// A lost stop response cannot be settled using canUse/canRecycle. A second
/// stop is a new explicit action, not an automatic replay of the first request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseRetry {
    Never,
    ExplicitAfterUnknownResult,
}

pub struct LeaseObservation {
    pub lease: EffectLease,
    /// True only after new successful reads and an acknowledged registration.
    /// This is a business authorization result, not an audio-engine receipt.
    pub can_use: bool,
    pub should_release: bool,
}
