//! Published music records, bound to an account and verified file digest.
mod cold;
mod detached;
mod records;
mod service;
pub use cold::{ColdOfflineAudio, ColdOfflineAudioPage, ColdOfflineAudioRow, ColdOfflineCursor};
pub(crate) use cold::{list_audio, open_audio};
pub(crate) use detached::{detach_download_queue, move_inline_download_queue};
pub use records::{OfflineCheck, OfflineRecord, PublishedDownload};
pub use service::OfflineLibrary;
pub(in crate::download) use service::verify as verify_detached_audio;
