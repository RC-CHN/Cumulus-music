use crate::{
    Result,
    download::{completed::FileOptions, file_identity::Identity, service::DownloadStatus},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectorySettings {
    pub root: PathBuf,
    pub path_type: u8,
    pub file_type: u8,
    pub revision: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Quiescing,
    Moving,
    RollingBack,
    Complete,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemPhase {
    Pending,
    Complete,
    RolledBack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationItem {
    pub task_id: String,
    pub operation_id: String,
    pub rollback_id: String,
    pub working: bool,
    pub source: Option<PathBuf>,
    pub target: Option<PathBuf>,
    pub phase: ItemPhase,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MigrationView {
    pub id: String,
    pub source: PathBuf,
    pub target: PathBuf,
    pub phase: MigrationPhase,
    pub items: Vec<MigrationItem>,
    pub skipped: Vec<String>,
    pub waiting: Vec<String>,
    pub required_bytes: u64,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Batch {
    pub view: MigrationView,
    pub source_identity: Identity,
    pub target_identity: Identity,
    pub before: Option<DirectorySettings>,
    pub original: BTreeMap<String, DownloadStatus>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryEvent {
    Intent,
    Planned,
    ItemComplete,
    RollbackItem,
    BeforeFinish,
    Finished,
}
pub type DirectoryObserver =
    Arc<dyn Fn(DirectoryEvent, &MigrationView) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct DirectoryOptions {
    pub files: FileOptions,
    pub observer: Option<DirectoryObserver>,
    /// Space to retain after the conservative full-copy estimate.
    pub reserve_bytes: u64,
}
