use super::{Inner, OWNER, ROOTS, WatchEvent, WatchRoot, WatchStatus};
use crate::{Error, Result, storage::OperationLock};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

impl Inner {
    pub(super) fn run_once(&self) -> Result<Vec<WatchRoot>> {
        let _lock = match OperationLock::acquire(self.store.directory(), OWNER, "library-monitor") {
            Ok(lock) => lock,
            Err(Error::MutationBusy) => return Ok(vec![]),
            Err(error) => return Err(error),
        };
        let mut results = Vec::new();
        for record in self.roots()? {
            if self.stopped() {
                break;
            }
            if !record.enabled {
                continue;
            }
            let began = self.store.transaction(|tx| {
                let Some(raw) = tx.get(OWNER, ROOTS, &record.id)? else {
                    return Ok(false);
                };
                let mut current: WatchRoot = serde_json::from_value(raw)?;
                if !current.enabled || current.revision != record.revision {
                    return Ok(false);
                }
                current.status = WatchStatus::Scanning;
                tx.put(OWNER, ROOTS, &record.id, &serde_json::to_value(current)?)?;
                Ok(true)
            })?;
            if !began {
                continue;
            }
            let cancelled = || {
                self.stopped()
                    || !self
                        .current(&record.id)
                        .ok()
                        .flatten()
                        .is_some_and(|r| r.enabled && r.revision == record.revision)
            };
            let scan = match fs::metadata(&record.path) {
                Ok(meta) if meta.is_dir() => {
                    self.library
                        .scan_with_progress(&record.path, &cancelled, &|value| {
                            if let Ok(mut progress) = self.progress.lock() {
                                progress.insert(record.id.clone(), (record.revision, value));
                            }
                        })
                }
                Ok(_) => Err(Error::InvalidInput("watch root is no longer a directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.library.mark_root_missing(&record.path, &cancelled)
                }
                Err(error) => Err(error.into()),
            };
            self.progress
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .remove(&record.id);
            let now = u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::State("system clock precedes epoch"))?
                    .as_millis(),
            )
            .map_err(|_| Error::State("system clock overflow"))?;
            let completed = self.store.transaction(|tx| {
                let Some(raw) = tx.get(OWNER, ROOTS, &record.id)? else {
                    return Ok(None);
                };
                let mut root: WatchRoot = serde_json::from_value(raw)?;
                if !root.enabled || root.revision != record.revision {
                    return Ok(None);
                }
                match scan {
                    Ok(value) => {
                        root.status = WatchStatus::Ready;
                        root.last_scan_ms = Some(now);
                        root.result = Some(value);
                        root.error = None;
                    }
                    Err(Error::Cancelled | Error::MutationBusy) => {
                        root.status = WatchStatus::Pending;
                        root.error = None;
                    }
                    Err(error) => {
                        root.status = WatchStatus::Error;
                        root.last_scan_ms = Some(now);
                        root.error = Some(error.to_string());
                    }
                }
                tx.put(OWNER, ROOTS, &record.id, &serde_json::to_value(&root)?)?;
                Ok(Some(root))
            })?;
            if let Some(root) = completed {
                self.emit(WatchEvent::Root {
                    root: Box::new(root.clone()),
                });
                results.push(root);
            }
        }
        Ok(results)
    }
}
