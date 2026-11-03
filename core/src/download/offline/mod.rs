//! Published music records, bound to an account and verified file digest.
mod cold;
mod detached;
mod records;
mod service;
pub use cold::ColdOfflineAudio;
pub(crate) use cold::open_plain_audio;
pub(crate) use detached::detach_download_queue;
pub use records::{OfflineCheck, OfflineRecord, PublishedDownload};
pub use service::OfflineLibrary;
