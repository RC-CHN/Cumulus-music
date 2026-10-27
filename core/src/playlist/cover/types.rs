use super::*;
use crate::{cloud::nos::NosAllocation, playlist::PlaylistPhase};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverPhase {
    Draft,
    Prepared,
    Allocating,
    AllocationRejected,
    Uploading,
    Uploaded,
    Binding,
    Confirmed,
    Rejected,
    Abandoned,
}
impl CoverPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Rejected | Self::Abandoned)
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverTaskView {
    pub id: String,
    pub playlist_id: String,
    pub phase: CoverPhase,
    pub crop: Option<CoverCrop>,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub total: u64,
    pub transfer: Option<crate::cloud::nos::NosStatus>,
    pub binding: Option<PlaylistPhase>,
    pub error: Option<String>,
    /// A cancelled attempt may leave an unreferenced object. No object deletion
    /// endpoint was recovered; never tell a host it has been removed remotely.
    pub may_leave_object: bool,
    pub cleanup_pending: bool,
}
/// Private journal. Image bytes are retained in SQLite until terminal cleanup,
/// allowing an interrupted local file materialization to restart safely. Scoped
/// upload tokens and original bytes must never cross the native boundary.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub id: String,
    pub playlist_id: String,
    pub revision: String,
    pub width: u32,
    pub height: u32,
    pub source: String,
    pub source_sha256: String,
    pub crop: Option<CoverCrop>,
    pub encoded: String,
    pub md5: String,
    pub sha256: String,
    pub total: u64,
    pub phase: CoverPhase,
    pub allocation: Option<NosAllocation>,
    pub binding_id: String,
    pub binding: Option<PlaylistPhase>,
    pub error: Option<String>,
    pub may_leave_object: bool,
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid playlist cover task ID"));
    }
    Ok(())
}
impl Task {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        valid_id(&self.binding_id)?;
        crate::protocol::api::identifier(&serde_json::json!(self.playlist_id))?;
        if self.revision.len() != 64
            || !self.revision.bytes().all(|b| b.is_ascii_hexdigit())
            || self.width < 320
            || self.height < 320
            || u64::from(self.width) * u64::from(self.height) > 16_000_000
            || self.source.len() > image::INPUT_LIMIT.div_ceil(3) * 4
            || self.encoded.len() > image::OUTPUT_LIMIT.div_ceil(3) * 4
            || self.total > image::OUTPUT_LIMIT as u64
        {
            return Err(Error::State("invalid persisted playlist cover task"));
        }
        let digest = |value: &str, length: usize| {
            value.len() == length
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if !digest(&self.source_sha256, 64)
            || (!self.phase.terminal() && self.source.is_empty())
            || (self.crop.is_some()
                && (self.total == 0 || !digest(&self.md5, 32) || !digest(&self.sha256, 64)))
            || (!self.phase.terminal()
                && self.phase != CoverPhase::Draft
                && (self.crop.is_none() || self.encoded.is_empty()))
            || (matches!(
                self.phase,
                CoverPhase::Uploading
                    | CoverPhase::Uploaded
                    | CoverPhase::Binding
                    | CoverPhase::Confirmed
                    | CoverPhase::Rejected
            ) && self.allocation.is_none())
        {
            return Err(Error::State("incomplete persisted cover phase"));
        }
        if let Some(crop) = self.crop {
            crop.validate(self.width, self.height)?;
        }
        if let Some(allocation) = &self.allocation {
            allocation.validate()?;
            if allocation.bucket() != "yyimgs" {
                return Err(Error::State("playlist cover uses another bucket"));
            }
            crate::protocol::api::identifier(&serde_json::json!(allocation.resource_id()))?;
        }
        Ok(())
    }
}
