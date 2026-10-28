use crate::{
    client::dto::{AlbumCard, EpisodeView, PodcastCard, TrackView},
    history::{HistoryAction, HistoryKind, HistoryMutationPhase, RecordOrigin},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub kind: HistoryKind,
    pub revision: String,
    pub sync_enabled: bool,
    pub can_sync: bool,
    pub remote_included: bool,
    pub remote_fetched_at: Option<u64>,
    pub local_count: usize,
    pub entries: Vec<HistoryEntry>,
    pub operations: Vec<HistoryOperationView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub played_at: u64,
    pub device: &'static str,
    pub origin: RecordOrigin,
    pub play_progress: Option<f64>,
    pub listened: Option<bool>,
    pub resource: HistoryResource,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryResource {
    Song { track: TrackView },
    Album { album: AlbumCard },
    Playlist { playlist: HistoryPlaylistCard },
    Voice { voice: EpisodeView },
    Podcast { radio: PodcastCard },
    Audiobook { radio: PodcastCard },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPlaylistCard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub creator: String,
    pub cover_url: Option<String>,
    pub track_count: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOperationView {
    pub id: String,
    pub action: &'static str,
    pub selected_count: Option<usize>,
    pub phase: HistoryMutationPhase,
    pub created_at: u64,
    pub local_removed: usize,
    pub remote_planned: bool,
    pub last_read_at: Option<u64>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryCommand {
    CancelUnseen {
        id: String,
        expected_revision: String,
        action: HistoryAction,
    },
    SetSync {
        enabled: bool,
    },
    Mutate {
        id: String,
        expected_revision: String,
        action: HistoryAction,
    },
    Reconcile {
        id: String,
    },
    AcceptCurrent {
        id: String,
        expected_revision: String,
    },
}
