use super::super::dto::TrackView;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemberQuery {
    Home,
    History { date: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberRightsView {
    pub server_now: f64,
    pub refresh_after_ms: Option<f64>,
    pub entitlements: Vec<MemberEntitlement>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberEntitlement {
    pub kind: &'static str,
    pub name: &'static str,
    pub active: bool,
    pub expires_at: f64,
    pub level: Option<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberPageView {
    pub revision: String,
    pub cached: bool,
    pub fresh: bool,
    pub fetched_at: f64,
    pub warning: Option<&'static str>,
    pub page: MemberPage,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemberPage {
    Home {
        modules: Vec<MemberModule>,
        messages: Vec<String>,
    },
    History {
        dates: Vec<String>,
        date: Option<String>,
        tracks: Vec<TrackView>,
    },
}
#[derive(Serialize)]
pub struct MemberModule {
    pub key: String,
    pub title: String,
    pub content: MemberContent,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemberContent {
    Banners { cards: Vec<MemberCard> },
    Tracks { tracks: Vec<TrackView> },
    Rankings { groups: Vec<MemberRanking> },
    Albums { cards: Vec<MemberCard> },
    Playlists { cards: Vec<MemberCard> },
    AudioBooks { cards: Vec<MemberCard> },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberCard {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover_url: Option<String>,
    pub action: Option<MemberTarget>,
}
#[derive(Serialize)]
pub struct MemberRanking {
    pub id: String,
    pub title: String,
    pub tracks: Vec<TrackView>,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemberTarget {
    Song { id: String },
    Playlist { id: String },
    Album { id: String },
    Radio { id: String },
    External { key: String },
}
