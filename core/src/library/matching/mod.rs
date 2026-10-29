//! Official M/2304 matching queue and M/23 metadata protocol, with native
//! account isolation, revision checks and durable per-batch recovery.
mod api;
mod hints;
pub(crate) use hints::file_metadata_hint;
mod service;
mod state;
pub use api::{MatchQuery, MatchingApi, normalize_matches};
pub use hints::{FileMetadataProbe, MatchHint, MetadataProbe, decode_metadata};
pub use service::LocalMatching;
pub use state::{MatchAction, MatchJob, MatchPhase};
