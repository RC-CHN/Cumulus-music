use crate::{
    account::resources::MutationPhase,
    podcast::{PodcastAction, PodcastScope},
};
use serde::{Deserialize, Serialize};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastState {
    pub liked_ids: Option<Vec<String>>,
    pub operations: Vec<PodcastMutationView>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodcastQuery {
    pub scope: PodcastScope,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub offset: usize,
    pub limit: usize,
    pub revision: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastCard {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub creator: String,
    pub description: String,
    pub program_count: u64,
    pub subscribed: Option<bool>,
    pub fee_type: i64,
    pub unavailable: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeView {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub creator: String,
    pub radio_id: Option<String>,
    pub radio_name: String,
    pub duration_ms: u64,
    pub fee_type: i64,
    pub liked: bool,
    pub unavailable: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastPage {
    pub epoch: String,
    pub scope: PodcastScope,
    pub revision: String,
    pub cards: Vec<PodcastCard>,
    pub episodes: Vec<EpisodeView>,
    pub total: usize,
    pub offset: usize,
    pub remote_total: u64,
    pub radio: Option<PodcastCard>,
    pub liked_ids: Vec<String>,
    pub resume_enabled: Option<bool>,
    pub operations: Vec<PodcastMutationView>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastMutationView {
    pub action: PodcastAction,
    pub id: String,
    pub desired: bool,
    pub observed: Option<bool>,
    pub phase: MutationPhase,
    pub acknowledged: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastCommand {
    Change {
        action: PodcastAction,
        id: String,
        desired: bool,
    },
    Reconcile {
        action: PodcastAction,
        id: String,
    },
}
