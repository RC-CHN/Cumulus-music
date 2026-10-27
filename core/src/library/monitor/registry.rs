use super::{Inner, OWNER, ROOTS, WatchEvent, WatchRoot, WatchStatus};
use crate::{Error, Result, library::identity::file_id};
use std::path::Path;

impl Inner {
    pub(super) fn roots(&self) -> Result<Vec<WatchRoot>> {
        let mut roots = self
            .store
            .values(OWNER, ROOTS)?
            .into_values()
            .map(|v| serde_json::from_value(v).map_err(Error::from))
            .collect::<Result<Vec<WatchRoot>>>()?;
        let progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
        for root in &mut roots {
            root.progress = progress
                .get(&root.id)
                .filter(|(revision, _)| {
                    *revision == root.revision
                        && root.enabled
                        && root.status == WatchStatus::Scanning
                })
                .map(|(_, value)| value.clone());
        }
        Ok(roots)
    }
    pub(super) fn watch(&self, path: &Path) -> Result<WatchRoot> {
        let path = path.canonicalize()?;
        if !path.is_dir() || path.starts_with(self.store.directory().canonicalize()?) {
            return Err(Error::InvalidInput(
                "watch root must be a directory outside core state",
            ));
        }
        let id = file_id(&path)?;
        let record = self.store.transaction(|tx| {
            let old: Option<WatchRoot> = tx
                .get(OWNER, ROOTS, &id)?
                .map(serde_json::from_value)
                .transpose()?;
            let revision = old
                .map(|v| v.revision)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::State("watch revision exhausted"))?;
            let record = WatchRoot {
                id: id.clone(),
                path,
                enabled: true,
                revision,
                status: WatchStatus::Pending,
                last_scan_ms: None,
                result: None,
                error: None,
                progress: None,
            };
            tx.put(OWNER, ROOTS, &id, &serde_json::to_value(&record)?)?;
            Ok(record)
        })?;
        self.notify()?;
        self.emit(WatchEvent::Root {
            root: Box::new(record.clone()),
        });
        Ok(record)
    }
    pub(super) fn unwatch(&self, id: &str) -> Result<Option<WatchRoot>> {
        let record = self.store.transaction(|tx| {
            let Some(value) = tx.get(OWNER, ROOTS, id)? else {
                return Ok(None);
            };
            let mut root: WatchRoot = serde_json::from_value(value)?;
            root.enabled = false;
            root.status = WatchStatus::Disabled;
            root.revision = root
                .revision
                .checked_add(1)
                .ok_or(Error::State("watch revision exhausted"))?;
            tx.put(OWNER, ROOTS, id, &serde_json::to_value(&root)?)?;
            Ok(Some(root))
        })?;
        self.notify()?;
        if let Some(root) = &record {
            self.emit(WatchEvent::Root {
                root: Box::new(root.clone()),
            });
        }
        Ok(record)
    }
    pub(super) fn current(&self, id: &str) -> Result<Option<WatchRoot>> {
        self.store
            .get(OWNER, ROOTS, id)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Error::from)
    }
}
