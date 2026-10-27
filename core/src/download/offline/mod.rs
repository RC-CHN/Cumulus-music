//! Published music records, bound to an account and verified file digest.
mod detached;
mod records;
mod service;
pub(crate) use detached::detach_download_queue;
pub use records::{OfflineCheck, OfflineRecord, PublishedDownload};
pub use service::OfflineLibrary;
