//! Official myFavorite and own followed-artist page reads. These paginated
//! views are not complete membership snapshots for mutation reconciliation.
mod api;
mod query;
mod service;
pub use api::{CollectionApi, CollectionData};
pub use query::CollectionQuery;
pub use service::{Collections, SavedCollectionPage};
