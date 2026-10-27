use super::super::dto::TrackView;
use crate::library::ScanProgress;
pub use crate::library::catalog::{LocalGroup, LocalGroupEntry, LocalQuery, LocalSort};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackView {
    pub track: TrackView,
    pub revision: String,
    pub file_name: String,
    pub folder: String,
    pub format: String,
    pub cue: bool,
    pub encrypted: bool,
    pub unreadable: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRootView {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    pub status: crate::library::monitor::WatchStatus,
    pub progress: Option<ScanProgress>,
    pub last_scan_ms: Option<u64>,
    pub updated: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub failures: usize,
    pub root_missing: bool,
    pub error: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPageView {
    pub revision: String,
    pub tracks: Vec<LocalTrackView>,
    pub groups: Vec<LocalGroupEntry>,
    pub roots: Vec<LocalRootView>,
    pub total: usize,
    pub playable: usize,
    pub offset: usize,
    pub more: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalCommand {
    Watch { path: String },
    Disable { id: String },
    Enable { id: String },
    Rescan,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLinkView {
    pub id: String,
    pub revision: String,
    pub song_id: String,
    pub track: Option<TrackView>,
    pub source: Option<crate::library::MappingSource>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMatchView {
    pub job: Option<crate::library::matching::MatchJob>,
    pub links: Vec<LocalLinkView>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalMatchCommand {
    Start {
        query: LocalQuery,
    },
    Control {
        id: String,
        action: crate::library::matching::MatchAction,
    },
    Bind {
        id: String,
        revision: String,
        song_id: String,
    },
    Clear {
        id: String,
        revision: String,
    },
}
