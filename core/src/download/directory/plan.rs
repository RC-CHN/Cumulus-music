use super::*;
use crate::download::{completed::fs, service::LyricsStatus};
use std::{collections::HashSet, fs as stdfs, path::PathBuf};
impl DownloadDirectory {
    /// Acquire every task and raw-transfer lock before constructing the manifest.
    /// Busy workers retain ownership; callers receive exact waiting task IDs.
    pub(super) fn task_locks(
        &self,
        batch: &mut types::Batch,
    ) -> Result<Option<Vec<OperationLock>>> {
        let mut locks = vec![];
        batch.view.waiting.clear();
        for id in batch.original.keys() {
            for namespace in ["download-task", "media-transfer"] {
                match OperationLock::acquire(
                    self.files.store.directory(),
                    namespace,
                    &serde_json::to_string(&(&self.files.owner, id))?,
                ) {
                    Ok(lock) => locks.push(lock),
                    Err(Error::MutationBusy) => {
                        if !batch.view.waiting.contains(id) {
                            batch.view.waiting.push(id.clone());
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(if batch.view.waiting.is_empty() {
            Some(locks)
        } else {
            None
        })
    }
    pub(super) fn plan(
        &self,
        batch: &mut types::Batch,
        token: &Option<SessionToken>,
    ) -> Result<bool> {
        let _files = OperationLock::acquire(self.files.store.directory(), "download-files", "all")?;
        let Some(_locks) = self.task_locks(batch)? else {
            self.save(batch, token)?;
            return Ok(false);
        };
        self.guard(batch, token)?;
        let mut planned = batch.clone();
        let mut destinations = HashSet::<PathBuf>::new();
        let mut tasks = Vec::new();
        for id in batch.original.keys() {
            let mut task = self.files.task(id)?;
            if task.file_operation.is_some() {
                return Err(Error::State("task has a pending file operation"));
            }
            if task.status == DownloadStatus::Cancelled
                || !task.root.starts_with(&batch.view.source)
            {
                planned.view.skipped.push(id.clone());
                continue;
            }
            let working = task.status != DownloadStatus::Complete || task.cleanup_pending;
            if task.target.is_none()
                && let crate::download::service::DownloadDestination::Path { path } =
                    &task.destination
            {
                let requested = paths::mapped(path, &batch.view.source, &batch.view.target)?;
                paths::prepare_target(&requested, &batch.view.target)?;
                fs::absent(&requested)?;
                if !destinations.insert(requested) {
                    return Err(Error::State("migration destinations collide"));
                }
            }
            let mut pairs = Vec::new();
            if working {
                let new_work = paths::mapped(&task.work, &batch.view.source, &batch.view.target)?;
                paths::prepare_target(&new_work, &batch.view.target)?;
                match stdfs::symlink_metadata(&new_work) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                    Ok(m) => {
                        if !m.is_dir() || stdfs::read_dir(&new_work)?.next().is_some() {
                            return Err(Error::State("destination working directory is occupied"));
                        }
                    }
                }
                for input in
                    self.files
                        .working_inputs(&task, &batch.view.source, &batch.view.target)?
                {
                    pairs.push((input.source, input.target));
                }
                if let Some(path) = &task.target {
                    let target = paths::mapped(path, &batch.view.source, &batch.view.target)?;
                    if !pairs.iter().any(|(_, t)| *t == target) {
                        fs::absent(&target)?;
                    }
                }
            } else {
                let source = task
                    .target
                    .clone()
                    .ok_or(Error::State("completed task lacks target"))?;
                pairs.push((
                    source.clone(),
                    paths::mapped(&source, &batch.view.source, &batch.view.target)?,
                ));
                if task.lyrics_status == LyricsStatus::Complete {
                    let source = source.with_extension("lrc");
                    match stdfs::symlink_metadata(&source) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                        Err(e) => return Err(e.into()),
                        Ok(_) => pairs.push((
                            source.clone(),
                            paths::mapped(&source, &batch.view.source, &batch.view.target)?,
                        )),
                    }
                }
            }
            for (source, target) in pairs {
                let meta = stdfs::symlink_metadata(&source)?;
                if !meta.is_file() {
                    return Err(Error::State("migration source is not a regular file"));
                }
                if !destinations.insert(target.clone()) {
                    return Err(Error::State("migration destinations collide"));
                }
                paths::prepare_target(&target, &batch.view.target)?;
                fs::absent(&target)?;
                planned.view.required_bytes = planned
                    .view
                    .required_bytes
                    .checked_add(meta.len())
                    .ok_or(Error::State("migration size overflow"))?;
            }
            planned.view.items.push(MigrationItem {
                task_id: id.clone(),
                operation_id: new_id(),
                rollback_id: new_id(),
                working,
                source: task.target.clone(),
                target: task
                    .target
                    .as_deref()
                    .map(|p| paths::mapped(p, &batch.view.source, &batch.view.target))
                    .transpose()?,
                phase: ItemPhase::Pending,
            });
            let original = serde_json::to_value(&task)?;
            if matches!(
                task.status,
                DownloadStatus::Queued
                    | DownloadStatus::Authorizing
                    | DownloadStatus::Downloading
                    | DownloadStatus::Processing
            ) {
                task.status = DownloadStatus::Paused;
            }
            tasks.push((original, task));
        }
        let available = paths::available(&batch.view.target)?;
        if planned
            .view
            .required_bytes
            .checked_add(self.options.reserve_bytes)
            .is_none_or(|n| n > available)
        {
            return Err(Error::State(
                "insufficient available space for directory migration",
            ));
        }
        planned.view.phase = MigrationPhase::Moving;
        planned.view.error = None;
        self.guard(batch, token)?;
        self.files.guarded(token, || {
            self.files.store.transaction(|tx| {
                for (original, task) in tasks {
                    if tx.get(&self.files.owner, TASKS, &task.id)?.as_ref() != Some(&original) {
                        return Err(Error::State("task changed during migration preflight"));
                    }
                    tx.put(
                        &self.files.owner,
                        TASKS,
                        &task.id,
                        &serde_json::to_value(&task)?,
                    )?;
                }
                tx.put(
                    &self.files.owner,
                    BATCHES,
                    &planned.view.id,
                    &serde_json::to_value(&planned)?,
                )
            })
        })?;
        *batch = planned;
        self.event(DirectoryEvent::Planned, batch)?;
        Ok(true)
    }
}
