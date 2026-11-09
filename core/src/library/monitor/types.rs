use crate::library::ScanReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatus {
    Pending,
    Scanning,
    Ready,
    Error,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchRoot {
    pub id: String,
    pub path: PathBuf,
    #[serde(default)]
    pub(super) order: u64,
    #[serde(default = "checked_default")]
    pub checked: bool,
    // Once unselected, old observations stay excluded until a checked
    // root publishes a fresh native scan. Never set by ordinary pause.
    #[serde(default)]
    pub(super) deselected: bool,
    pub enabled: bool,
    pub revision: u64,
    pub status: WatchStatus,
    pub last_scan_ms: Option<u64>,
    pub result: Option<ScanReport>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::library::ScanProgress>,
}

fn checked_default() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchEvent {
    Root { root: Box<WatchRoot> },
    Error { message: String },
}
