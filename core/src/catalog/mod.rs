//! Search, recommendations and catalog resources (official M/15 and M/612).
mod api;
mod artist;
pub mod rankings;
pub use api::{CatalogApi, PlayOptions, SearchOptions, SongBatch};
pub use artist::{ArtistApi, ArtistPageRequest};
