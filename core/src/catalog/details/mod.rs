//! Account-bound artist/album read state. Snapshots are recovery data, not grants.
mod query;
mod request;
mod service;
pub use query::CatalogQuery;
pub(crate) use service::album_duration;
pub use service::{CatalogDetails, SavedCatalogPage};
