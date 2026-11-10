//! Official playlist contracts and account-bound persistent mutation recovery.
mod api;
pub mod cover;
pub(crate) mod external_import;
mod external_input;
mod extras;
mod local_membership;
mod service;
mod snapshot;
mod types;
pub(crate) use api::{MGC_TRIAL, ZonePlaylistMembership};
pub use api::{PlaylistApi, PlaylistTracks, TrackOperation};
pub use external_input::{
    ExternalPlaylistInput, ExternalPlaylistSource, MAX_EXTERNAL_PLAYLIST_LINKS,
    preview_external_playlist_inputs,
};
pub use local_membership::{
    PlaylistLocalChange, PlaylistLocalMembership, PlaylistLocalMutation, PlaylistLocalRef,
    PlaylistLocalRow, PlaylistLocalStore, PlaylistMixedOrderRow, PlaylistMixedRemoveReceipt,
    PlaylistMixedReorderEvidence, PlaylistMixedReorderReceipt,
};
pub use service::PlaylistService;
pub(crate) use service::{GroupDownloadLikedOrigin, GroupDownloadPlaylistProof};
pub(crate) use snapshot::playlist_entry_id;
pub use snapshot::{PlaylistList, PlaylistSnapshot, PlaylistSummary};
pub use types::*;

pub(crate) mod offline_snapshot;

pub(crate) const OFFLINE_SNAPSHOT_KIND: &str = "playlist-offline-snapshot-v1";
pub(crate) const OFFLINE_DELETED_KIND: &str = "playlist-offline-deleted-v1";
