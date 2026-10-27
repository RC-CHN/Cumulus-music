//! Search, recommendations and catalog resources (official M/15 and M/612).
mod api;
pub mod rankings;
pub use api::{CatalogApi, PlayOptions, SearchOptions, SongBatch};
