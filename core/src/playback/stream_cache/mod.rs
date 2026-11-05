//! Private process-owned cache of streaming audio bytes. Entries are never
//! download records, offline grants or authorization. Only an independently
//! authorized native source can obtain an admission.
pub(crate) mod range;
mod reader;
mod store;
#[cfg(test)]
mod tests;

use range::HttpsRanges;
pub use reader::StreamingAudioReader;
use store::{Admission, BLOCK, Cache, Fill, Key, Lease};

use crate::{Result, storage::StateStore};
use std::sync::Arc;

pub(crate) struct StreamingCache {
    pub(crate) store: Arc<Cache>,
    pub(crate) backend: Arc<dyn range::RangeMedia>,
}
impl StreamingCache {
    pub(crate) fn open(state: &StateStore, capacity_bytes: u64) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Cache::open(state, capacity_bytes)?,
            backend: Arc::new(HttpsRanges::new()?),
        }))
    }
}

pub(crate) use store::Key as StreamingCacheKey;
