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
    pub enabled: bool,
    pub revision: u64,
    pub status: WatchStatus,
    pub last_scan_ms: Option<u64>,
    pub result: Option<ScanReport>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::library::ScanProgress>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchEvent {
    Root { root: Box<WatchRoot> },
    Error { message: String },
}
