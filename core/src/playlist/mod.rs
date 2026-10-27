//! Official playlist contracts and account-bound persistent mutation recovery.
mod api;
pub mod cover;
mod extras;
mod service;
mod snapshot;
mod types;
pub use api::{PlaylistApi, PlaylistTracks, TrackOperation};
pub use service::PlaylistService;
pub use snapshot::{PlaylistList, PlaylistSnapshot, PlaylistSummary};
pub use types::*;
