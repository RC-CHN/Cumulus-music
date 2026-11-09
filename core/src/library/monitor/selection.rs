//! Persistent checked directories, distinct from temporarily paused scanning.
use super::{Inner, OWNER, ROOTS, WatchEvent, WatchRoot, WatchStatus};
use crate::{Error, Result, storage::StateTransaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

const LIMIT: usize = 128;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySelection {
    pub revision: String,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub id: String,
    pub path: String,
    pub checked: bool,
    pub paused: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryChoice {
    pub id: String,
    pub checked: bool,
}

pub(super) fn records(tx: &StateTransaction<'_>) -> Result<Vec<WatchRoot>> {
    let mut roots = tx
        .values(OWNER, ROOTS)?
        .into_iter()
        .map(|(id, value)| {
            let root: WatchRoot = serde_json::from_value(value)?;
            if root.id != id {
                return Err(Error::State("local directory ID mismatch"));
            }
            Ok(root)
        })
        .collect::<Result<Vec<_>>>()?;
    roots.sort_by(|a, b| a.order.cmp(&b.order).then(a.path.cmp(&b.path)));
    Ok(roots)
}

pub(super) fn next_order(roots: &[WatchRoot]) -> Result<u64> {
    roots
        .last()
        .map_or(0, |root| root.order)
        .checked_add(1)
        .ok_or(Error::State("local directory order exhausted"))
}

fn revision(roots: &[WatchRoot]) -> Result<String> {
    let membership = roots
        .iter()
        .map(|r| (&r.id, &r.path, r.order, r.checked, r.enabled, r.revision))
        .collect::<Vec<_>>();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(
        &membership,
    )?)))
}

fn view(roots: &[WatchRoot]) -> Result<DirectorySelection> {
    Ok(DirectorySelection {
        revision: revision(roots)?,
        entries: roots
            .iter()
            .map(|r| DirectoryEntry {
                id: r.id.clone(),
                path: r.path.to_string_lossy().into_owned(),
                checked: r.checked,
                paused: r.checked && !r.enabled,
            })
            .collect(),
    })
}

impl Inner {
    pub(super) fn directory_selection(&self) -> Result<DirectorySelection> {
        self.store.transaction(|tx| view(&records(tx)?))
    }

    pub(super) fn stage_directory(
        &self,
        path: &Path,
        expected: &str,
    ) -> Result<DirectorySelection> {
        let (path, id) = self.checked_path(path)?;
        self.store.transaction(|tx| {
            let mut roots = records(tx)?;
            if revision(&roots)? != expected {
                return Err(Error::QueueChanged);
            }
            if !roots.iter().any(|r| r.id == id) {
                if roots.len() >= LIMIT {
                    return Err(Error::InvalidInput("too many local music directories"));
                }
                let order = next_order(&roots)?;
                let root = WatchRoot {
                    id,
                    path,
                    order,
                    checked: false,
                    deselected: false,
                    enabled: false,
                    revision: 1,
                    status: WatchStatus::Disabled,
                    last_scan_ms: None,
                    result: None,
                    error: None,
                    progress: None,
                };
                tx.put(OWNER, ROOTS, &root.id, &serde_json::to_value(&root)?)?;
                roots.push(root);
            }
            view(&roots)
        })
    }

    pub(super) fn apply_directories(
        &self,
        expected: &str,
        choices: Vec<DirectoryChoice>,
    ) -> Result<DirectorySelection> {
        if choices.len() > LIMIT {
            return Err(Error::InvalidInput("too many local music directories"));
        }
        let input_len = choices.len();
        let choices: BTreeMap<_, _> = choices.into_iter().map(|v| (v.id, v.checked)).collect();
        let (view, changed) = self.store.transaction(|tx| {
            let mut roots = records(tx)?;
            if revision(&roots)? != expected
                || roots.len() != input_len
                || roots.len() != choices.len()
                || roots.iter().any(|r| !choices.contains_key(&r.id))
            {
                return Err(Error::QueueChanged);
            }
            let mut changed = Vec::new();
            for root in &mut roots {
                let checked = choices[&root.id];
                if checked == root.checked {
                    continue;
                }
                root.checked = checked;
                root.deselected |= !checked;
                root.enabled = checked;
                root.revision = root
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("watch revision exhausted"))?;
                root.status = if checked {
                    WatchStatus::Pending
                } else {
                    WatchStatus::Disabled
                };
                root.last_scan_ms = None;
                root.result = None;
                root.error = None;
                tx.put(OWNER, ROOTS, &root.id, &serde_json::to_value(&root)?)?;
                changed.push(root.clone());
            }
            Ok((view(&roots)?, changed))
        })?;
        if !changed.is_empty() {
            self.notify()?;
            for root in changed {
                self.emit(WatchEvent::Root {
                    root: Box::new(root),
                });
            }
        }
        Ok(view)
    }
}

/// Projection fence for catalog reads from the same native snapshot. Scanner
/// facts and already-owned playback resources are retained; only rows under
/// an unchecked root and no overlapping checked root become unavailable here.
#[derive(Clone, Default)]
pub(crate) struct DirectoryScope {
    included: HashSet<PathBuf>,
    excluded: HashSet<PathBuf>,
}
impl DirectoryScope {
    pub(crate) fn read(tx: &StateTransaction<'_>) -> Result<Self> {
        let mut scope = Self::default();
        for root in records(tx)? {
            if root.checked && !root.deselected {
                scope.included.insert(root.path);
            } else if root.deselected {
                scope.excluded.insert(root.path);
            }
        }
        Ok(scope)
    }

    pub(crate) fn permits(&self, path: &Path) -> bool {
        !path.ancestors().any(|p| self.excluded.contains(p))
            || path.ancestors().any(|p| self.included.contains(p))
    }
}
