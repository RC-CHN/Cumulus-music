//! Search, recommendations and catalog resources (official M/15 and M/612).
mod album;
mod api;
mod artist;
pub mod details;
pub mod metadata;
mod privileges;
pub mod rankings;
pub use album::{AlbumDetailSource, AlbumDetails, AlbumReadState};
pub use api::{CatalogApi, PlayOptions, SearchOptions, SongBatch};
pub use artist::{ArtistApi, ArtistPageRequest};

/// Official M/21 R chooses id || albumId before converting to decimal text.
pub(crate) fn album_identifier(value: &serde_json::Value) -> crate::Result<String> {
    let id = &value["id"];
    let absent = id.is_null() || id == "" || id == false || id.as_f64() == Some(0.0);
    crate::protocol::api::identifier(if absent { &value["albumId"] } else { id })
}
