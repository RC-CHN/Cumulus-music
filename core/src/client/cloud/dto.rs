use super::*;
use crate::client::dto::TrackView;
use crate::cloud::library::{DeleteOperation, DeletePhase};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudTab {
    Uploaded,
    Uploading,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSort {
    Natural,
    Uploaded,
    Title,
    Artist,
    Album,
    Format,
    Size,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudQuery {
    pub tab: CloudTab,
    pub keyword: String,
    pub sort: CloudSort,
    pub descending: bool,
    pub offset: usize,
    pub limit: usize,
}
impl CloudQuery {
    pub(super) fn validate(&self) -> Result<()> {
        if !(1..=200).contains(&self.limit)
            || self.offset > 200_000
            || self.keyword.chars().count() > 256
        {
            return Err(Error::InvalidInput("invalid cloud page bounds or filter"));
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTrackView {
    pub track: TrackView,
    pub file_name: String,
    pub file_size: u64,
    pub added_at: u64,
    pub bitrate: u64,
    pub format: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudUploadView {
    pub id: String,
    pub title: String,
    pub file_name: String,
    pub phase: CloudPhase,
    pub bytes: u64,
    pub total: u64,
    pub label: &'static str,
    pub enabled: bool,
    pub running: bool,
    pub error: Option<String>,
    pub can_resume: bool,
    pub can_pause: bool,
    pub can_cancel: bool,
    pub can_reconcile: bool,
    pub can_renew: bool,
    pub can_dismiss: bool,
    pub attachments: Vec<AttachmentView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeleteView {
    pub id: String,
    pub phase: DeletePhase,
    pub total: usize,
    pub removed: usize,
    pub remaining: usize,
}
impl From<DeleteOperation> for CloudDeleteView {
    fn from(op: DeleteOperation) -> Self {
        Self {
            id: op.id,
            phase: op.phase,
            total: op.ids.len(),
            removed: op.removed.len(),
            remaining: op.remaining.len(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudPageView {
    pub epoch: String,
    pub revision: Option<String>,
    pub updated_ms: Option<u64>,
    pub size: Option<u64>,
    pub max_size: Option<u64>,
    pub uploaded_count: usize,
    pub uploading_count: usize,
    pub total: usize,
    pub offset: usize,
    pub tracks: Vec<CloudTrackView>,
    pub uploads: Vec<CloudUploadView>,
    pub deletions: Vec<CloudDeleteView>,
    pub worker_error: Option<&'static str>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudUploadInput {
    pub path: PathBuf,
    pub cover: Option<PathBuf>,
    pub lyric: Option<PathBuf>,
    pub cue: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudCommand {
    PauseAll,
    ResumeAll {
        retry_rejected: bool,
    },
    Upload {
        files: Vec<CloudUploadInput>,
    },
    Pause {
        ids: Vec<String>,
    },
    Resume {
        ids: Vec<String>,
        retry_rejected: bool,
    },
    Cancel {
        ids: Vec<String>,
    },
    Dismiss {
        ids: Vec<String>,
    },
    Renew {
        id: String,
    },
    ReconcileUpload {
        id: String,
    },
    Delete {
        ids: Vec<String>,
        revision: String,
    },
    ReconcileDelete {
        id: String,
    },
}
#[derive(Serialize)]
pub struct CloudFailure {
    pub id: String,
    pub message: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResult {
    pub epoch: String,
    pub accepted: Vec<String>,
    pub failed: Vec<CloudFailure>,
    pub deletion: Option<CloudDeleteView>,
}
