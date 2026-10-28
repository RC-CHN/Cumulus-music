use crate::{
    Result,
    catalog::{ArtistPageRequest, artist::video_cursor},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogQuery {
    Artist {
        id: String,
    },
    ArtistDynamic {
        id: String,
    },
    ArtistSongs {
        id: String,
        offset: usize,
        limit: usize,
    },
    ArtistAlbums {
        id: String,
        offset: usize,
        limit: usize,
    },
    ArtistIntroduction {
        id: String,
    },
    SimilarArtists {
        id: String,
        offset: usize,
        limit: usize,
    },
    ArtistVideos {
        id: String,
        cursor: String,
        size: usize,
    },
    ArtistConcerts {
        id: String,
        offset: usize,
        limit: usize,
    },
    Album {
        id: String,
    },
}
impl CatalogQuery {
    pub fn id(&self) -> &str {
        match self {
            Self::Artist { id }
            | Self::ArtistDynamic { id }
            | Self::ArtistSongs { id, .. }
            | Self::ArtistAlbums { id, .. }
            | Self::ArtistIntroduction { id }
            | Self::SimilarArtists { id, .. }
            | Self::ArtistVideos { id, .. }
            | Self::ArtistConcerts { id, .. }
            | Self::Album { id } => id,
        }
    }
    pub fn validate(&self) -> Result<()> {
        identifier(&json!(self.id()))?;
        match self {
            Self::ArtistSongs { offset, limit, .. }
            | Self::ArtistAlbums { offset, limit, .. }
            | Self::SimilarArtists { offset, limit, .. }
            | Self::ArtistConcerts { offset, limit, .. } => ArtistPageRequest {
                offset: *offset,
                limit: *limit,
            }
            .validate(),
            Self::ArtistVideos { cursor, size, .. } => {
                ArtistPageRequest {
                    offset: 0,
                    limit: *size,
                }
                .validate()?;
                video_cursor(cursor)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
