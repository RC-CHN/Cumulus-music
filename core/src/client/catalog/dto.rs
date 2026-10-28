use super::super::dto::{ArtistReference, TrackView};
use crate::catalog::details::CatalogQuery;
pub use crate::catalog::metadata::{ConcertDate as CatalogDate, VideoKind as CatalogVideoKind};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub query: CatalogQuery,
    pub fetched_at: u64,
    pub cached: bool,
    pub revision: String,
    pub page: CatalogPageView,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CatalogPageView {
    Artist {
        artist: ArtistCard,
        tracks: Vec<TrackView>,
        more: bool,
    },
    ArtistDynamic {
        followed: bool,
        online_count: u64,
        video_count: Option<u64>,
    },
    ArtistSongs {
        offset: usize,
        total: u64,
        more: bool,
        tracks: Vec<TrackView>,
    },
    ArtistAlbums {
        offset: usize,
        total: u64,
        more: bool,
        albums: Vec<AlbumCard>,
    },
    ArtistIntroduction {
        brief_description: String,
        sections: Vec<CatalogIntroduction>,
    },
    SimilarArtists {
        offset: usize,
        more: bool,
        artists: Vec<ArtistCard>,
    },
    ArtistVideos {
        next_cursor: String,
        more: bool,
        size: u64,
        videos: Vec<CatalogVideoCard>,
    },
    ArtistConcerts {
        offset: usize,
        total: u64,
        more: bool,
        concerts: Vec<ConcertCard>,
    },
    Album {
        album: AlbumCard,
        read: Option<crate::catalog::AlbumReadState>,
        tracks: Vec<TrackView>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCard {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub cover_url: Option<String>,
    pub description: String,
    pub song_count: u64,
    pub album_count: u64,
    pub mv_count: Option<u64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumCard {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub translations: Vec<String>,
    pub description: String,
    pub artists: Vec<ArtistReference>,
    pub cover_url: Option<String>,
    pub track_count: u64,
    pub published_at: Option<i64>,
    pub subscribed: Option<bool>,
    pub explicit: bool,
}
#[derive(Serialize)]
pub struct CatalogIntroduction {
    pub title: String,
    pub text: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVideoCard {
    pub id: String,
    pub kind: CatalogVideoKind,
    pub title: String,
    pub description: String,
    pub artists: Vec<ArtistReference>,
    pub cover_url: Option<String>,
    pub duration_ms: u64,
    pub play_count: u64,
    /// Only a verified decimal ID with explicit MV classification.
    pub mv_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcertCard {
    pub id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub start: Option<CatalogDate>,
    pub end: Option<CatalogDate>,
    pub address: String,
    pub price: String,
}
