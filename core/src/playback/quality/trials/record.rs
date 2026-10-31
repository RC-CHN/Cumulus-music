use super::super::{QualityRight, QualityUseRight};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTrialPhase {
    Sending,
    Uncertain,
    Acknowledged,
    Active,
    Expired,
    Rejected,
}
impl QualityTrialPhase {
    pub(super) fn terminal(self) -> bool {
        matches!(self, Self::Expired | Self::Rejected)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityTrial {
    pub right: QualityRight,
    pub operation: String,
    pub revision: u64,
    pub phase: QualityTrialPhase,
    /// A query may confirm Active after a lost response. Keep the actual write
    /// receipt separate rather than manufacturing an acknowledgement.
    pub acknowledged: bool,
    pub observed: Option<QualityUseRight>,
    pub read_failed: bool,
}
impl QualityTrial {
    pub fn can_use(&self) -> bool {
        self.phase == QualityTrialPhase::Active
            && !self.read_failed
            && self.observed.as_ref().is_some_and(|r| {
                matches!(
                    r.phase(),
                    super::super::TrialPhase::Active | super::super::TrialPhase::Granted
                )
            })
    }
    pub(super) fn validate(&self, right: QualityRight) -> Result<()> {
        let valid_id = uuid::Uuid::parse_str(&self.operation)
            .is_ok_and(|id| id.simple().to_string() == self.operation);
        if self.right != right
            || !valid_id
            || self.revision == 0
            || (self.phase == QualityTrialPhase::Acknowledged && !self.acknowledged)
            || (matches!(
                self.phase,
                QualityTrialPhase::Sending
                    | QualityTrialPhase::Uncertain
                    | QualityTrialPhase::Rejected
            ) && self.acknowledged)
            || (self.phase == QualityTrialPhase::Active
                && self.observed.as_ref().is_none_or(|r| !r.can_use))
        {
            return Err(Error::State("invalid quality trial journal"));
        }
        Ok(())
    }
    pub(super) fn advance(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::State("quality trial revision exhausted"))?;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrialConsent {
    DoNotStart,
    ExplicitStart,
}
