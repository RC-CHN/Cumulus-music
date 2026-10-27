use super::files::{Artifact, Identity};
use crate::{
    download::transfer::TransferIntegrity, media::integrity::FileIntegrity, membership::Quality,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputContainer {
    #[default]
    Audio,
    Ncm,
    Official,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadOptions {
    pub quality: Quality,
    pub tags: bool,
    pub cover: bool,
    pub lyrics: bool,
    pub container: OutputContainer,
}
impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            quality: Quality::Standard,
            tags: true,
            cover: true,
            lyrics: false,
            container: OutputContainer::Audio,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DownloadDestination {
    Path {
        path: PathBuf,
    },
    Named {
        root: PathBuf,
        path_type: u8,
        file_type: u8,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Authorizing,
    Downloading,
    Processing,
    Paused,
    Error,
    Complete,
    Cancelling,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    Authorize,
    Transfer,
    Prepare,
    Publish,
    Complete,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadAction {
    Pause,
    Cancel,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LyricsStatus {
    Pending,
    Prepared,
    Empty,
    Conflict,
    Complete,
    Error,
    Missing,
}

/// Native persistence record. Contains resource URLs; never expose directly
/// through IPC. Hosts should project a dedicated public progress DTO.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DownloadOrigin {
    #[default]
    Song,
    Cloud {
        file_name: String,
        file_size: u64,
    },
    Mv {
        resolution: u32,
    },
    Voice {
        voice_id: String,
    },
}
impl DownloadOrigin {
    pub fn is_cloud(&self) -> bool {
        matches!(self, Self::Cloud { .. })
    }
    pub fn is_mv(&self) -> bool {
        matches!(self, Self::Mv { .. })
    }
    pub fn is_voice(&self) -> bool {
        matches!(self, Self::Voice { .. })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub song_id: String,
    #[serde(default)]
    pub origin: DownloadOrigin,
    pub options: DownloadOptions,
    pub destination: DownloadDestination,
    pub target: Option<PathBuf>,
    pub work: PathBuf,
    pub status: DownloadStatus,
    pub phase: DownloadPhase,
    pub created_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_ms: Option<u64>,
    pub bytes: u64,
    pub authorization_count: u64,
    pub resource: Option<Value>,
    pub song: Option<Value>,
    pub encrypted: bool,
    pub official_encryption: bool,
    pub raw_integrity: Option<TransferIntegrity>,
    pub output_integrity: Option<FileIntegrity>,
    pub cleanup_pending: bool,
    pub last_error: Option<String>,
    pub service_code: Option<Value>,
    pub lyrics_status: LyricsStatus,
    pub lyrics_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_operation: Option<String>,
    pub(in crate::download) root: PathBuf,
    pub(in crate::download) root_identity: Identity,
    pub(in crate::download) work_identity: Identity,
    pub(in crate::download) target_parent: Option<Identity>,
    pub(in crate::download) prepared: Option<Artifact>,
    pub(in crate::download) cover: Option<Artifact>,
    pub(in crate::download) lyric: Option<Artifact>,
}
