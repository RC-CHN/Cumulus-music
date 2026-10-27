use super::{MediaUrl, files::FileToken};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResource {
    pub url: MediaUrl,
    pub size: Option<u64>,
    pub md5: Option<String>,
}
impl TransferResource {
    pub(super) fn validate(&mut self, limit: u64) -> Result<()> {
        if self.size.is_some_and(|n| n == 0 || n > limit) {
            return Err(Error::InvalidInput(
                "authorized media size exceeds transfer limit",
            ));
        }
        if let Some(md5) = &mut self.md5 {
            if md5.len() != 32 || !md5.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::InvalidInput("invalid media MD5"));
            }
            md5.make_ascii_lowercase();
        }
        Ok(())
    }
}
pub struct CreateTransfer {
    pub resource: TransferResource,
    pub target: PathBuf,
    pub resource_id: Option<String>,
    pub id: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Queued,
    Running,
    Paused,
    Error,
    Publishing,
    Complete,
    Cancelling,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferIntegrity {
    pub bytes: u64,
    pub md5: String,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: String,
    pub resource: TransferResource,
    pub resource_id: Option<String>,
    pub target: PathBuf,
    pub part: PathBuf,
    pub status: TransferStatus,
    pub bytes: u64,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub integrity: Option<TransferIntegrity>,
    pub cleanup_pending: bool,
    pub representation_restarted: bool,
    pub last_error: Option<String>,
    pub(in crate::download) parent_token: FileToken,
    pub(in crate::download) part_token: FileToken,
}
// Deliberately no Debug: URLs, resource IDs and validators are native state.
