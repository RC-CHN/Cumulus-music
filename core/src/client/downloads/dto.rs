use crate::{
    client::dto::TrackView,
    download::{
        completed::{FileAction, FileOperationView},
        directory::{DirectorySettings, MigrationView},
        service::{DownloadOptions, DownloadPhase, DownloadStatus, LyricsStatus},
    },
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadTab {
    #[default]
    Active,
    Completed,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadCategory {
    #[default]
    All,
    Song,
    Voice,
    Mv,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadSort {
    None,
    Name,
    Artist,
    Album,
    Size,
    #[default]
    Completed,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadQuery {
    pub tab: DownloadTab,
    #[serde(default)]
    pub category: DownloadCategory,
    pub keyword: String,
    pub sort: DownloadSort,
    pub descending: bool,
    pub offset: usize,
    pub limit: usize,
}
impl Default for DownloadQuery {
    fn default() -> Self {
        Self {
            tab: DownloadTab::Active,
            category: DownloadCategory::All,
            keyword: String::new(),
            sort: DownloadSort::Completed,
            descending: true,
            offset: 0,
            limit: 100,
        }
    }
}
impl DownloadQuery {
    // A growing prefix is returned atomically: refreshing or extending it cannot
    // stitch together pages from different download orders. Other catalog APIs
    // retain their 100-item page limit.
    pub(super) fn validate(&self) -> crate::Result<()> {
        if !(1..=1_000_000).contains(&self.limit) || self.offset > 1_000_000 {
            return Err(crate::Error::InvalidInput("invalid download window bounds"));
        }
        if self.keyword.len() > 1000 {
            return Err(crate::Error::InvalidInput("download keyword too long"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskView {
    pub id: String,
    pub source: &'static str,
    pub track: TrackView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_voice_id: Option<String>,
    pub collection_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_podcast: Option<DownloadVoicePodcastView>,
    pub status: DownloadStatus,
    pub phase: DownloadPhase,
    pub bytes: u64,
    pub total: Option<u64>,
    pub progress: Option<f64>,
    /// Native measured bytes per second; absent outside this session's live transfer.
    pub speed_bytes_per_second: Option<u64>,
    pub requested_quality: String,
    pub actual_quality: Option<String>,
    pub requested_resolution: Option<u32>,
    pub actual_resolution: Option<u32>,
    pub encrypted: bool,
    pub path: Option<String>,
    pub created_ms: u64,
    pub completed_ms: Option<u64>,
    pub cleanup_pending: bool,
    pub needs_transfer_restart: bool,
    pub file_operation: Option<String>,
    pub lyrics_status: LyricsStatus,
    pub error: Option<&'static str>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadShareAccess {
    Allowed,
    Unavailable,
    AlbumRequired,
}

#[derive(Clone, Serialize)]
pub struct DownloadVoicePodcastView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJobView {
    pub id: String,
    pub kind: &'static str,
    pub running: bool,
    pub error: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsView {
    pub epoch: String,
    pub tasks: Vec<DownloadTaskView>,
    pub total: usize,
    pub active_count: usize,
    pub completed_count: usize,
    pub offset: usize,
    pub directory: Option<DirectorySettings>,
    pub options: DownloadOptions,
    pub migration: Option<MigrationView>,
    pub operations: Vec<FileOperationView>,
    pub jobs: Vec<DownloadJobView>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskControl {
    Pause,
    Resume,
    Cancel,
    Repair,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DownloadCommand {
    RestartFailedTransfer {
        task_id: String,
    },
    DeleteFiles {
        task_ids: Vec<String>,
    },
    ConfirmVoice(super::VoiceDownloadConfirmation),
    StartMv {
        id: String,
        resolution: u32,
    },
    StartCloud {
        song_ids: Vec<String>,
        revision: String,
        lyrics: bool,
    },
    Start {
        song_ids: Vec<String>,
        options: DownloadOptions,
    },
    Control {
        task_ids: Vec<String>,
        action: TaskControl,
    },
    All {
        action: TaskControl,
    },
    ConfirmClear {
        receipt: String,
    },
    Preferences {
        revision: String,
        options: DownloadOptions,
    },
    Configure {
        root: String,
        path_type: u8,
        file_type: u8,
    },
    ChangeDirectory {
        root: String,
        #[serde(deserialize_with = "explicit_directory_revision")]
        revision: Option<String>,
    },
    Migrate {
        root: String,
    },
    Migration {
        operation_id: String,
        cancel: bool,
    },
    File {
        task_id: String,
        action: FileAction,
        target: Option<String>,
    },
    Rename {
        task_id: String,
        name: String,
    },
    MoveToDirectory {
        task_id: String,
        root: String,
    },
    FileOperation {
        operation_id: String,
        cancel: bool,
    },
    StopOperation {
        operation_id: String,
    },
}
fn explicit_directory_revision<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
#[derive(Serialize)]
pub struct DownloadDirectoryPreview {
    pub epoch: String,
    #[serde(flatten)]
    pub choice: crate::download::directory::DirectoryChoice,
}
#[derive(Serialize)]
pub struct DownloadClearPlan {
    pub epoch: String,
    pub receipt: Option<String>,
    pub count: usize,
}
#[derive(Serialize)]
pub struct DownloadFailure {
    pub id: String,
    pub message: &'static str,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadSkipReason {
    Downloaded,
    Local,
    Waiting,
    Working,
    Cancelling,
    OptionsChanged,
    Existing,
    Missing,
    Conflict,
}
#[derive(Serialize)]
pub struct DownloadSkip {
    pub id: String,
    pub reason: DownloadSkipReason,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub epoch: String,
    pub accepted: Vec<String>,
    pub failed: Vec<DownloadFailure>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<DownloadSkip>,
    pub operation_id: Option<String>,
}
