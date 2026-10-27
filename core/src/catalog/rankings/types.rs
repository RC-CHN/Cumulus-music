use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RankingQuery {
    Home,
    AlbumSales { year: i32 },
    AlbumYear { year: i32 },
    SongSales,
    Album { id: String },
}
impl RankingQuery {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::AlbumSales { year } | Self::AlbumYear { year }
                if !(2018..=time::OffsetDateTime::now_utc().year() + 1).contains(year) =>
            {
                Err(Error::InvalidInput("invalid sales ranking year"))
            }
            Self::Album { id } => identifier(&json!(id)).map(|_| ()),
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingTarget {
    Playlist,
    AlbumSales,
    SongSales,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingPreview {
    pub id: String,
    pub rank: Option<i64>,
    pub last_rank: Option<i64>,
    pub name: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub is_album: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingCard {
    pub id: String,
    pub target: RankingTarget,
    pub name: String,
    pub name_show_style: String,
    pub update_frequency: String,
    pub can_play: bool,
    pub cover_url: Option<String>,
    pub text_cover: bool,
    pub tracks: Vec<RankingPreview>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingGroup {
    pub name: String,
    pub display_type: String,
    pub cards: Vec<RankingCard>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSale {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub rank: Option<i64>,
    pub rank_incr: Option<i64>,
    pub newest: bool,
    pub sale_num: Option<u64>,
    pub certification: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesBoard {
    pub period: String,
    pub year: Option<i32>,
    pub singles: bool,
    pub products: Vec<AlbumSale>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSale {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub quantity: Option<u64>,
    pub indicator: Option<f64>,
    pub rank_incr: Option<i64>,
    pub newest: bool,
    /// Native metadata, never serialized directly to a frontend.
    pub song: Option<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RankingPage {
    Home {
        groups: Vec<RankingGroup>,
    },
    AlbumSales {
        year: i32,
        boards: Vec<SalesBoard>,
    },
    SongSales {
        generated_at: Option<u64>,
        songs: Vec<SongSale>,
    },
    Album {
        id: String,
        album: Value,
        songs: Vec<Value>,
    },
}
