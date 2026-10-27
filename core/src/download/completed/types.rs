use super::fs::Facts;
use crate::{
    Result,
    download::{file_identity::Identity, service::DownloadTask},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Move,
    Delete,
    Forget,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilePhase {
    Prepared,
    Cleanup,
    Complete,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStage {
    Pending,
    Copying,
    Ready,
    Cleaned,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileEvent {
    Prepared,
    WaitingForPlayback,
    CopyChunk,
    Staged,
    DestinationReady,
    MetadataCommitted,
    SourceRemoved,
    Finished,
}

/// Native-host progress/cancellation hook, invoked outside SQLite/account locks.
pub type FileObserver = Arc<dyn Fn(FileEvent, &FileOperationView) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct FileOptions {
    /// Also useful when a destination filesystem cannot link the original inode.
    pub copy_only: bool,
    pub observer: Option<FileObserver>,
}

#[derive(Clone, Serialize)]
pub struct FileEntryView {
    pub role: String,
    pub source: PathBuf,
    pub destination: Option<PathBuf>,
    pub stage: FileStage,
    pub bytes_copied: u64,
    pub replacement_retained: bool,
}
#[derive(Clone, Serialize)]
pub struct FileOperationView {
    pub id: String,
    pub task_id: String,
    pub action: FileAction,
    pub phase: FilePhase,
    pub missing: Vec<String>,
    pub files: Vec<FileEntryView>,
    pub error: Option<String>,
    pub working: bool,
    pub old_work_retained: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ManagedFile {
    pub role: String,
    pub source: PathBuf,
    pub source_parent: Identity,
    pub expected: Facts,
    pub destination: Option<PathBuf>,
    pub destination_parent: Option<Identity>,
    pub destination_facts: Option<Facts>,
    pub temporary: Option<PathBuf>,
    pub temporary_identity: Option<Identity>,
    pub stage: FileStage,
    pub bytes_copied: u64,
    pub replacement_retained: bool,
    #[serde(default)]
    pub alias: Option<usize>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Operation {
    pub id: String,
    pub task_id: String,
    pub action: FileAction,
    pub phase: FilePhase,
    #[serde(default)]
    pub migration_id: Option<String>,
    #[serde(default)]
    pub working: Option<super::working::Working>,
    pub task: DownloadTask,
    pub target: Option<PathBuf>,
    pub files: Vec<ManagedFile>,
    pub missing: Vec<String>,
    pub error: Option<String>,
}
impl Operation {
    pub fn view(&self) -> FileOperationView {
        FileOperationView {
            id: self.id.clone(),
            task_id: self.task_id.clone(),
            action: self.action,
            phase: self.phase,
            missing: self.missing.clone(),
            error: self.error.clone(),
            working: self.working.is_some(),
            old_work_retained: self.working.as_ref().is_some_and(|w| w.old_work_retained),
            files: self
                .files
                .iter()
                .map(|f| FileEntryView {
                    role: f.role.clone(),
                    source: f.source.clone(),
                    destination: f.destination.clone(),
                    stage: f.stage,
                    bytes_copied: f.bytes_copied,
                    replacement_retained: f.replacement_retained,
                })
                .collect(),
        }
    }
}
