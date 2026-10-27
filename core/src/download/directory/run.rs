use super::*;
use crate::download::service::{DownloadAction, TASK_CONTROL};
impl DownloadDirectory {
    pub fn resume(&self, id: &str) -> Result<MigrationView> {
        self.run(id, false)
    }
    pub fn cancel(&self, id: &str) -> Result<MigrationView> {
        self.run(id, true)
    }
    fn run(&self, id: &str, cancel: bool) -> Result<MigrationView> {
        let token = self.files.token()?;
        let _lock = gate::lock(&self.files.store, &self.files.owner)?;
        self.files.guarded(&token, || Ok(()))?;
        let mut batch = self.read(id)?;
        if batch.view.phase == MigrationPhase::Complete {
            if cancel {
                return Err(Error::State(
                    "completed migration requires an explicit reverse migration",
                ));
            }
            return Ok(batch.view);
        }
        if batch.view.phase == MigrationPhase::Cancelled {
            return Ok(batch.view);
        }
        self.guard(&batch, &token)?;
        if cancel && batch.view.phase != MigrationPhase::RollingBack {
            let mut cancelling = batch.clone();
            cancelling.view.phase = MigrationPhase::RollingBack;
            self.save(&cancelling, &token)?;
            batch = cancelling;
        }
        let result = if batch.view.phase == MigrationPhase::RollingBack {
            self.rollback(&mut batch, &token)
        } else {
            self.forward(&mut batch, &token)
        };
        if result.is_err() {
            // Old-owner journal records responsibility even after account switch.
            batch.view.error = Some("migration_conflict_or_interruption".into());
            self.files.store.put(
                &self.files.owner,
                BATCHES,
                id,
                &serde_json::to_value(&batch)?,
            )?;
        }
        result?;
        self.files.guarded(&token, || Ok(()))?;
        Ok(batch.view)
    }
    fn forward(&self, batch: &mut types::Batch, token: &Option<SessionToken>) -> Result<()> {
        if batch.view.phase == MigrationPhase::Quiescing && !self.plan(batch, token)? {
            return Ok(());
        }
        let children = self.children(batch, token);
        for i in 0..batch.view.items.len() {
            self.guard(batch, token)?;
            let item = &batch.view.items[i];
            if item.phase == ItemPhase::Complete {
                continue;
            }
            if !children.has_operation(&item.operation_id)? {
                if item.working {
                    children.start_work(
                        &item.task_id,
                        &batch.view.source,
                        &batch.view.target,
                        &item.operation_id,
                    )?;
                } else {
                    children.start_planned(
                        &item.task_id,
                        item.target
                            .clone()
                            .ok_or(Error::State("missing completed target"))?,
                        &item.operation_id,
                        &batch.view.target,
                    )?;
                }
            }
            if children.resume(&item.operation_id)?.phase != FilePhase::Complete {
                return Err(Error::State("migration child did not complete"));
            }
            batch.view.items[i].phase = ItemPhase::Complete;
            self.save(batch, token)?;
            self.event(DirectoryEvent::ItemComplete, batch)?;
        }
        self.finish(batch, token, false)
    }
    fn rollback(&self, batch: &mut types::Batch, token: &Option<SessionToken>) -> Result<()> {
        let children = self.children(batch, token);
        for i in (0..batch.view.items.len()).rev() {
            self.guard(batch, token)?;
            let item = &batch.view.items[i];
            if item.phase == ItemPhase::RolledBack {
                continue;
            }
            if children.has_operation(&item.operation_id)? {
                let mut op = children.get(&item.operation_id)?;
                if op.phase == FilePhase::Prepared {
                    op = children.cancel(&op.id)?;
                }
                if op.phase == FilePhase::Cleanup {
                    op = children.resume(&op.id)?;
                }
                if op.phase == FilePhase::Complete {
                    if !children.has_operation(&item.rollback_id)? {
                        if item.working {
                            children.start_work(
                                &item.task_id,
                                &batch.view.target,
                                &batch.view.source,
                                &item.rollback_id,
                            )?;
                        } else {
                            children.start_planned(
                                &item.task_id,
                                item.source
                                    .clone()
                                    .ok_or(Error::State("missing rollback target"))?,
                                &item.rollback_id,
                                &batch.view.source,
                            )?;
                        }
                    }
                    if children.resume(&item.rollback_id)?.phase != FilePhase::Complete {
                        return Err(Error::State("rollback child did not complete"));
                    }
                }
            }
            batch.view.items[i].phase = ItemPhase::RolledBack;
            self.save(batch, token)?;
            self.event(DirectoryEvent::RollbackItem, batch)?;
        }
        self.finish(batch, token, true)
    }
    fn finish(
        &self,
        batch: &mut types::Batch,
        token: &Option<SessionToken>,
        cancelled: bool,
    ) -> Result<()> {
        let Some(_locks) = self.task_locks(batch)? else {
            self.save(batch, token)?;
            return Ok(());
        };
        self.guard(batch, token)?;
        self.event(DirectoryEvent::BeforeFinish, batch)?;
        self.guard(batch, token)?;
        let mut finished = batch.clone();
        finished.view.phase = if cancelled {
            MigrationPhase::Cancelled
        } else {
            MigrationPhase::Complete
        };
        finished.view.error = None;
        self.files.guarded(token, || {
            self.files.store.transaction(|tx| {
                if tx.get(&self.files.owner, SETTINGS, "active")?
                    != Some(serde_json::to_value(&batch.view.id)?)
                    || tx.get(&self.files.owner, SETTINGS, "settings")?
                        != batch
                            .before
                            .as_ref()
                            .map(serde_json::to_value)
                            .transpose()?
                {
                    return Err(Error::State("directory settings changed during migration"));
                }
                let settings = if cancelled {
                    batch.before.clone()
                } else {
                    let old = batch.before.as_ref();
                    Some(DirectorySettings {
                        root: batch.view.target.clone(),
                        path_type: old.map_or(0, |s| s.path_type),
                        file_type: old.map_or(0, |s| s.file_type),
                        revision: new_id(),
                    })
                };
                if let Some(settings) = settings {
                    tx.put(
                        &self.files.owner,
                        SETTINGS,
                        "settings",
                        &serde_json::to_value(settings)?,
                    )?;
                } else {
                    tx.delete(&self.files.owner, SETTINGS, "settings")?;
                }
                for (id, original) in &batch.original {
                    let Some(value) = tx.get(&self.files.owner, TASKS, id)? else {
                        continue;
                    };
                    let mut task: DownloadTask = serde_json::from_value(value)?;
                    if matches!(
                        task.status,
                        DownloadStatus::Complete | DownloadStatus::Cancelled
                    ) {
                        continue;
                    }
                    let action: Option<DownloadAction> = tx
                        .get(&self.files.owner, TASK_CONTROL, id)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    if action == Some(DownloadAction::Cancel) {
                        task.status = DownloadStatus::Cancelling;
                    } else if action == Some(DownloadAction::Pause) {
                        task.status = DownloadStatus::Paused;
                    } else if task.status == DownloadStatus::Paused
                        && matches!(
                            original,
                            DownloadStatus::Queued
                                | DownloadStatus::Authorizing
                                | DownloadStatus::Downloading
                                | DownloadStatus::Processing
                        )
                    {
                        task.status = DownloadStatus::Queued;
                    }
                    tx.put(&self.files.owner, TASKS, id, &serde_json::to_value(task)?)?;
                }
                tx.delete(&self.files.owner, SETTINGS, "active")?;
                tx.put(
                    &self.files.owner,
                    BATCHES,
                    &batch.view.id,
                    &serde_json::to_value(&finished)?,
                )
            })
        })?;
        *batch = finished;
        self.event(DirectoryEvent::Finished, batch)
    }
}
