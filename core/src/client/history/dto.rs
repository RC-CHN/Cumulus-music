use crate::{
    client::dto::{AlbumCard, EpisodeView, PodcastCard, TrackView},
    comments::ResourceKind,
    history::{HistoryAction, HistoryKind, HistoryMutationPhase, RecordOrigin},
};
use serde::{Deserialize, Serialize};

/// Selected row identities from the ordered, account-bound history view.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySelectionScope {
    pub revision: String,
    pub ids: Vec<String>,
}

/// The identities frozen by the table at menu opening, separate from playback.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoryRowMenuTarget {
    pub history_kind: HistoryKind,
    pub revision: String,
    pub clicked_id: String,
    pub selected_ids: Vec<String>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRowMenuInfo {
    pub comment: Option<HistoryRowComment>,
    pub copy_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_file: Option<HistoryVoiceFileMenu>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryVoiceFileMenu {
    Download,
    Reveal { receipt: String },
    Unavailable,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRowComment {
    pub resource: HistoryRowCommentResource,
    pub count: Option<u64>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRowCommentResource {
    pub kind: ResourceKind,
    pub id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub kind: HistoryKind,
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_revision: Option<String>,
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
    /// Snapshot-only M556 menu availability, separate from admission on click.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_menu_copyright_blocked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<HistoryRadioChapterView>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub can_collect_radio: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRadioChapterView {
    pub name: String,
    pub tag: Option<String>,
    pub recent: bool,
    pub percent: Option<u8>,
    pub completed: bool,
    pub playable: bool,
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

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryRadioCollection {
    Toggle { expected: bool },
    Reconcile,
}
