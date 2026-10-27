use crate::{account::resources::MutationPhase, mv::playback::VideoStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvCard {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub cover_url: Option<String>,
    pub duration_ms: u64,
    pub play_count: u64,
    pub subscribed: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvView {
    pub card: MvCard,
    pub description: String,
    pub published_date: String,
    pub play_resolutions: Vec<u32>,
    pub downloads: Vec<MvDownloadChoice>,
    pub cached: bool,
    pub liked: Option<bool>,
    pub like_count: u64,
    pub comment_count: u64,
    pub like_operation: Option<MvLikeView>,
    pub related_songs: Vec<crate::client::dto::TrackView>,
    pub related_privilege_failed: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvLikeView {
    pub id: String,
    pub desired: bool,
    pub observed: Option<bool>,
    pub count: Option<u64>,
    pub phase: MutationPhase,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvRecommendationsView {
    pub id: String,
    pub items: Vec<MvRecommendationView>,
    pub cached: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvRecommendationView {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub cover_url: Option<String>,
    pub duration_ms: u64,
    pub play_count: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvDownloadChoice {
    pub resolution: u32,
    pub estimated_bytes: Option<u64>,
    pub points: u64,
    pub available: bool,
    pub reason: Option<&'static str>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MvQuery {
    pub keyword: String,
    pub offset: usize,
    pub limit: usize,
    pub revision: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvPage {
    pub epoch: String,
    pub revision: String,
    pub cards: Vec<MvCard>,
    pub total: usize,
    pub offset: usize,
    pub operations: Vec<MvMutationView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MvMutationView {
    pub id: String,
    pub desired: bool,
    pub observed: Option<bool>,
    pub phase: MutationPhase,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoView {
    pub epoch: String,
    pub revision: String,
    pub play_id: String,
    pub id: Option<String>,
    pub state: VideoStatus,
    pub requested_resolution: u32,
    pub actual_resolution: Option<u32>,
    pub local: bool,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub speed: f64,
    pub error: Option<&'static str>,
}
// Separate IPC command type: internal Snapshot/Suspend are not frontend actions.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VideoAction {
    Open {
        id: String,
        resolution: u32,
        download_task_id: Option<String>,
    },
    Play,
    Pause {
        paused: bool,
    },
    Seek {
        seconds: f64,
    },
    Volume {
        value: f64,
    },
    Speed {
        value: f64,
    },
    Quality {
        resolution: u32,
    },
    Refresh,
    Stop,
}
