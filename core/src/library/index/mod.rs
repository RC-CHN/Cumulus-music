mod cue;
mod decoded;
mod mapping;
mod relocate;
mod scan;
mod view;
use super::{
    LibraryItem,
    probe::{AudioProbe, Ffprobe},
    records::CueRecord,
};
use crate::{
    Error, Result,
    storage::{OperationLock, StateStore},
};
pub use mapping::{LocalMapping, MappingSource};
pub(crate) use relocate::relocate_download;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

const OWNER: &str = "local";
const FILES: &str = "library_files_v1";
const CUES: &str = "library_cues_v1";
pub(crate) const MAPS: &str = "local_map_v1";

#[derive(Clone)]
pub struct LocalLibrary {
    store: Arc<StateStore>,
    probe: Arc<dyn AudioProbe>,
    state_root: PathBuf,
}
struct Index {
    files: BTreeMap<String, LibraryItem>,
    cues: BTreeMap<String, CueRecord>,
}

impl LocalLibrary {
    pub(super) fn state_store(&self) -> Arc<StateStore> {
        self.store.clone()
    }
    pub fn new(store: Arc<StateStore>) -> Result<Self> {
        Self::with_probe(store, Arc::new(Ffprobe::default()))
    }
    pub fn with_probe(store: Arc<StateStore>, probe: Arc<dyn AudioProbe>) -> Result<Self> {
        let state_root = store.directory().canonicalize()?;
        Ok(Self {
            store,
            probe,
            state_root,
        })
    }
    pub(crate) fn lock(&self) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), OWNER, "library-index")
    }
    fn load(&self) -> Result<Index> {
        self.store.transaction(|tx| {
            let files = tx
                .values(OWNER, FILES)?
                .into_iter()
                .map(|(key, value)| Ok((key, serde_json::from_value(value)?)))
                .collect::<Result<_>>()?;
            let cues = tx
                .values(OWNER, CUES)?
                .into_iter()
                .map(|(key, value)| Ok((key, serde_json::from_value(value)?)))
                .collect::<Result<_>>()?;
            Ok(Index { files, cues })
        })
    }
    fn commit(&self, old: &Index, new: &Index, cancelled: &dyn Fn() -> bool) -> Result<()> {
        // Cancellation may read watch-root state. Never call it while holding
        // the SQLite connection lock; committing is this operation's boundary.
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.store.transaction(|tx| {
            for (id, item) in &new.files {
                if old.files.get(id) != Some(item) {
                    tx.put(OWNER, FILES, id, &serde_json::to_value(item)?)?;
                }
            }
            for (id, item) in &new.cues {
                if old.cues.get(id) != Some(item) {
                    tx.put(OWNER, CUES, id, &serde_json::to_value(item)?)?;
                }
            }
            for id in old.files.keys().filter(|id| !new.files.contains_key(*id)) {
                tx.delete(OWNER, FILES, id)?;
            }
            for id in old.cues.keys().filter(|id| !new.cues.contains_key(*id)) {
                tx.delete(OWNER, CUES, id)?;
            }
            Ok(())
        })
    }
}
