//! Official playlist contracts and account-bound persistent mutation recovery.
mod api;
pub mod cover;
mod external_input;
mod extras;
mod service;
mod snapshot;
mod types;
pub use api::{PlaylistApi, PlaylistTracks, TrackOperation};
pub use external_input::{
    ExternalPlaylistInput, ExternalPlaylistSource, MAX_EXTERNAL_PLAYLIST_LINKS,
    preview_external_playlist_inputs,
};
pub use service::PlaylistService;
pub use snapshot::{PlaylistList, PlaylistSnapshot, PlaylistSummary};
pub use types::*;
