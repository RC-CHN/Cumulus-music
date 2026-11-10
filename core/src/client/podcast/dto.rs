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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastSearchMembership {
    pub subscribed_ids: Option<Vec<String>>,
    pub pending_ids: Vec<String>,
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
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastRadioOrder {
    Read {
        id: String,
    },
    Set {
        scope: PodcastScope,
        revision: String,
        ascending: bool,
    },
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_episode_count: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browse_playback_receipt: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodcastPurchasedTarget {
    pub kind: crate::podcast::PurchasedKind,
    pub id: String,
    pub revision: String,
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum PodcastPurchasedPlayCount {
    Number(u64),
    Compact(String),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastPurchasedRadio {
    #[serde(flatten)]
    pub card: PodcastCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_count: Option<PodcastPurchasedPlayCount>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "item", rename_all = "snake_case")]
pub enum PodcastPurchasedRow {
    Radio(PodcastPurchasedRadio),
    Episode(EpisodeView),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastPurchasedRows {
    pub rows: Vec<PodcastPurchasedRow>,
    pub next_offset: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastPage {
    pub epoch: String,
    pub scope: PodcastScope,
    pub revision: String,
    pub cards: Vec<PodcastCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribed_badge_receipt: Option<String>,
    pub episodes: Vec<EpisodeView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchases: Option<PodcastPurchasedRows>,
    pub total: usize,
    pub offset: usize,
    pub remote_total: u64,
    pub radio: Option<PodcastCard>,
    pub liked_ids: Vec<String>,
    /// False means no complete membership observation, even if the vector is empty.
    pub liked_ids_known: bool,
    pub resume_enabled: Option<bool>,
    pub audiobook_skip_enabled: Option<bool>,
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
    CollectRecommendation {
        id: String,
        receipt: String,
        revision: String,
    },
    ChangeSearchSubscription {
        id: String,
        expected: bool,
    },
    ChangeVinylSubscription {
        play_id: String,
        expected: bool,
    },
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
