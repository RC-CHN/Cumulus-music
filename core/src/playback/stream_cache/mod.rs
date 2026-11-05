//! Private process-owned cache of streaming audio bytes. Entries are never
//! download records, offline grants or authorization. Only an independently
//! authorized native source can obtain an admission.
pub(crate) mod range;
mod reader;
mod settings;
mod store;
#[cfg(test)]
mod tests;

use range::HttpsRanges;
pub use reader::StreamingAudioReader;
use settings::Settings;
pub use settings::StreamingMediaCacheStatus;
use store::{Admission, BLOCK, Cache, Fill, Key, Lease};

use crate::{Result, storage::StateStore};
use std::sync::Arc;

pub(crate) struct StreamingCache {
    pub(crate) store: Arc<Cache>,
    pub(crate) backend: Arc<dyn range::RangeMedia>,
    settings: Option<Settings>,
}
impl StreamingCache {
    pub(crate) fn open(state: &StateStore, capacity_bytes: u64) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Cache::open(state, capacity_bytes)?,
            backend: Arc::new(HttpsRanges::new()?),
            settings: None,
        }))
    }

    pub(crate) fn configured(state: Arc<StateStore>) -> Result<Arc<Self>> {
        let (settings, capacity) = Settings::open(state.clone())?;
        Ok(Arc::new(Self {
            store: Cache::open(&state, capacity)?,
            backend: Arc::new(HttpsRanges::new()?),
            settings: Some(settings),
        }))
    }

    pub(crate) fn status(&self) -> Result<Option<StreamingMediaCacheStatus>> {
        self.settings
            .as_ref()
            .map(|settings| settings.status(&self.store))
            .transpose()
    }

    pub(crate) fn set_capacity(
        &self,
        revision: &str,
        capacity_bytes: u64,
    ) -> Result<StreamingMediaCacheStatus> {
        self.settings()?.set(&self.store, revision, capacity_bytes)
    }

    pub(crate) fn clear_checked(&self, revision: &str) -> Result<StreamingMediaCacheStatus> {
        self.settings()?.clear(&self.store, Some(revision))
    }

    pub(crate) fn clear_native(&self) -> Result<()> {
        match &self.settings {
            Some(settings) => settings.clear(&self.store, None).map(|_| ()),
            None => self.store.clear(),
        }
    }

    fn settings(&self) -> Result<&Settings> {
        self.settings.as_ref().ok_or(crate::Error::State(
            "streaming cache preferences are unavailable",
        ))
    }
}

pub(crate) use store::Key as StreamingCacheKey;
