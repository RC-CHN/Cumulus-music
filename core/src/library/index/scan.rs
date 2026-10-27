use super::{Index, LocalLibrary};
use crate::{
    Error, Result,
    library::{
        LibraryItem, LocalRevision, ScanIssue, ScanProgress, ScanReport, ScanStage,
        identity::{fingerprint, path_id},
        supported_audio,
    },
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

fn check(cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn collect(
    path: &Path,
    excluded: &Path,
    cancelled: &dyn Fn() -> bool,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    check(cancelled)?;
    let meta = fs::symlink_metadata(path)?;
    if meta.is_symlink() || path.starts_with(excluded) {
        return Ok(());
    }
    if meta.is_file() {
        out.push(path.into());
    } else if meta.is_dir() {
        let mut children = fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for path in children {
            collect(&path, excluded, cancelled, out)?;
        }
    }
    Ok(())
}

impl LocalLibrary {
    pub(crate) fn mark_root_missing(
        &self,
        root: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ScanReport> {
        check(cancelled)?;
        if !root.is_absolute() {
            return Err(Error::InvalidInput("watch root must be absolute"));
        }
        let _lock = self.lock()?;
        match fs::metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
            Ok(_) => return Err(Error::State("watch root reappeared; rescan")),
        }
        let old = self.load()?;
        let mut new = Index {
            files: old.files.clone(),
            cues: old.cues.clone(),
        };
        let mut report = ScanReport {
            root_missing: true,
            ..Default::default()
        };
        for item in new.files.values_mut() {
            if item.path.starts_with(root) && !item.missing {
                item.missing = true;
                report.missing += 1;
            }
        }
        for item in new.cues.values_mut() {
            if item.path.starts_with(root) && !item.missing {
                item.missing = true;
                report.cue_missing += 1;
            }
        }
        self.commit(&old, &new, cancelled)?;
        Ok(report)
    }
    pub fn scan(&self, root: impl AsRef<Path>) -> Result<ScanReport> {
        self.scan_with(root, &|| false)
    }
    /// Traversal/probes stage a complete result. Cancellation or a denied
    /// subtree cannot publish a partial scan or mistaken deletion markers.
    pub fn scan_with(
        &self,
        root: impl AsRef<Path>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ScanReport> {
        self.scan_with_progress(root, cancelled, &|_| {})
    }
    /// Progress is observational. Only the final transaction publishes items;
    /// observers must not re-enter mutating library operations.
    pub fn scan_with_progress(
        &self,
        root: impl AsRef<Path>,
        cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(ScanProgress),
    ) -> Result<ScanReport> {
        check(cancelled)?;
        let _lock = self.lock()?;
        progress(ScanProgress {
            stage: ScanStage::Discovering,
            total: 0,
            processed: 0,
        });
        let root = root.as_ref().canonicalize()?;
        let mut paths = Vec::new();
        collect(&root, &self.state_root, cancelled, &mut paths)?;
        let total = paths
            .iter()
            .filter(|p| supported_audio(p) || is_cue(p))
            .count();
        let mut processed = 0;
        progress(ScanProgress {
            stage: ScanStage::Probing,
            total,
            processed,
        });
        let old = self.load()?;
        let mut new = Index {
            files: old.files.clone(),
            cues: old.cues.clone(),
        };
        let mut seen = HashSet::new();
        let mut cue_seen = HashSet::new();
        let mut report = ScanReport::default();
        for path in paths.iter().filter(|path| supported_audio(path)) {
            check(cancelled)?;
            let id = path_id(path)?;
            seen.insert(id.clone());
            let previous = new.files.get(&id);
            let build = (|| {
                let before = fingerprint(path)?;
                if previous.is_some_and(|item| {
                    item.available()
                        && item.revision
                            == LocalRevision::File {
                                fingerprint: before.clone(),
                            }
                }) {
                    return Ok(None);
                }
                let metadata = self.probe.inspect(path)?;
                check(cancelled)?;
                if !metadata.duration.is_finite() || metadata.duration < 0.0 {
                    return Err(Error::Media("invalid local audio duration"));
                }
                if before != fingerprint(path)? {
                    return Err(Error::State("file changed during metadata probe"));
                }
                Ok(Some(LibraryItem {
                    id: id.clone(),
                    path: path.clone(),
                    metadata,
                    revision: LocalRevision::File {
                        fingerprint: before,
                    },
                    missing: false,
                    unreadable: false,
                }))
            })();
            match build {
                Ok(Some(item)) => {
                    new.files.insert(id, item);
                    report.updated += 1;
                }
                Ok(None) => report.unchanged += 1,
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => {
                    report.errors.push(ScanIssue {
                        path: path.clone(),
                        error: error.to_string(),
                    });
                    if let Some(item) = new.files.get_mut(&id) {
                        item.unreadable = true;
                    }
                }
            }
            processed += 1;
            progress(ScanProgress {
                stage: ScanStage::Probing,
                total,
                processed,
            });
        }
        for (id, item) in &mut new.files {
            if item.path.starts_with(&root) && !seen.contains(id) && !item.missing {
                item.missing = true;
                report.missing += 1;
            }
        }
        for path in paths.iter().filter(|path| is_cue(path)) {
            check(cancelled)?;
            let id = path_id(path)?;
            cue_seen.insert(id.clone());
            match self.build_cue(path, new.cues.get(&id)) {
                Ok((changed, item)) => {
                    new.cues.insert(id, item);
                    if changed {
                        report.cue_updated += 1;
                    } else {
                        report.cue_unchanged += 1;
                    }
                }
                Err(error) => {
                    report.errors.push(ScanIssue {
                        path: path.clone(),
                        error: error.to_string(),
                    });
                    if let Some(item) = new.cues.get_mut(&id) {
                        item.unreadable = true;
                    }
                }
            }
            processed += 1;
            progress(ScanProgress {
                stage: ScanStage::Probing,
                total,
                processed,
            });
        }
        for (id, item) in &mut new.cues {
            if item.path.starts_with(&root) && !cue_seen.contains(id) && !item.missing {
                item.missing = true;
                report.cue_missing += 1;
            }
        }
        check(cancelled)?;
        progress(ScanProgress {
            stage: ScanStage::Committing,
            total,
            processed,
        });
        self.commit(&old, &new, cancelled)?;
        Ok(report)
    }
}

fn is_cue(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cue"))
}
