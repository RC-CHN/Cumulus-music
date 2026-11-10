//! Published music records, bound to an account and verified file digest.
mod cold;
mod detached;
mod records;
mod search_playback;
mod service;
pub use cold::{
    ColdOfflineAudio, ColdOfflineAudioPage, ColdOfflineAudioRow, ColdOfflineCursor,
    ColdOfflineFlacAuthority, ColdOfflineFlacReader,
};
pub(crate) use cold::{list_audio, open_audio};
pub(crate) use detached::{detach_download_queue, move_inline_download_queue};
pub use records::{OfflineCheck, OfflineRecord, PublishedDownload};
pub use search_playback::SearchPublication;
pub(crate) use search_playback::{HeldSearchFile, OFFLINE_PLAYLIST_DOWNLOAD, PlaylistDownload};
pub use service::OfflineLibrary;
pub(in crate::download) use service::verify as verify_detached_audio;
