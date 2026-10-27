use super::{
    control::{Control, Lifetime},
    files,
    manager::CONTROL,
    *,
};
use crate::{
    Error, Result,
    download::transfer::{RunOptions, TransferStatus},
    storage::OperationLock,
};
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone)]
pub struct DownloadRunOptions {
    pub byte_budget: Option<u64>,
    /// A host/scheduler stop pauses active tasks at checkpoints.
    pub stop: Arc<AtomicBool>,
}
impl Default for DownloadRunOptions {
    fn default() -> Self {
        Self {
            byte_budget: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl DownloadService {
    pub async fn run(&self, id: &str, options: DownloadRunOptions) -> Result<DownloadTask> {
        self.run_locked(id, options, None, self.session.token()?)
            .await
    }
    pub(super) async fn run_locked(
        &self,
        id: &str,
        options: DownloadRunOptions,
        scheduler_lock: Option<Arc<OperationLock>>,
        token: crate::account::SessionToken,
    ) -> Result<DownloadTask> {
        if options.byte_budget == Some(0) {
            return Err(Error::InvalidInput("download byte budget must be positive"));
        }
        self.guard(&token)?;
        let lock = Arc::new(self.lock(id)?);
        crate::download::directory::gate::require(&self.store, &self.owner, None)?;
        let task = self.read(id)?;
        if task.file_operation.is_some() {
            return Err(Error::State("resume the pending file operation first"));
        }
        let alive = Arc::new(AtomicBool::new(true));
        let _life = Lifetime(alive.clone());
        let gate = Control {
            service: self.clone(),
            token: token.clone(),
            id: id.into(),
            alive,
            stop: options.stop,
            _task_lock: lock,
            _scheduler_lock: scheduler_lock,
        };
        let result = self.execute(task, &gate, options.byte_budget).await;
        if let Err(error) = &result {
            let mut current = self.read(id)?;
            if !matches!(
                current.status,
                DownloadStatus::Complete | DownloadStatus::Cancelled | DownloadStatus::Cancelling
            ) {
                current.status = if matches!(error, Error::Cancelled | Error::StaleResponse) {
                    DownloadStatus::Paused
                } else {
                    DownloadStatus::Error
                };
                current.last_error = Some(error.to_string());
                current.service_code = match error {
                    Error::Service { code, .. } => Some(code.clone()),
                    Error::HttpStatus { status, .. } => Some((*status).into()),
                    _ => None,
                };
                // The old owner retains responsibility; never write to a new UID.
                self.save(&current)?;
            }
            if matches!(error, Error::Cancelled) && self.guard(&token).is_ok() {
                if gate.action()? == Some(DownloadAction::Cancel) && !files::published(&current)? {
                    self.cancel_task(&token, &mut current)?;
                }
                return Ok(current);
            }
        }
        result
    }
    async fn execute(
        &self,
        mut task: DownloadTask,
        gate: &Control,
        byte_budget: Option<u64>,
    ) -> Result<DownloadTask> {
        if task.status == DownloadStatus::Cancelled {
            self.cleanup(&gate.token, &mut task)?;
            return Ok(task);
        }
        if task.status == DownloadStatus::Cancelling {
            self.cancel_task(&gate.token, &mut task)?;
            return Ok(task);
        }
        if task.status == DownloadStatus::Complete {
            task = self.verify_complete(task, gate.clone()).await?;
        } else {
            // Publication recovery must not renew URLs or rewrite prepared bytes.
            if task.phase == DownloadPhase::Publish {
                if !files::published(&task)? {
                    gate.require()?;
                }
            } else {
                gate.require()?;
                let mut raw = if self
                    .store
                    .get(&self.owner, "media_transfers_v1", &task.id)?
                    .is_some()
                {
                    Some(self.transfer.get(&task.id)?)
                } else {
                    None
                };
                if raw
                    .as_ref()
                    .is_some_and(|r| r.status == TransferStatus::Publishing)
                {
                    raw = Some(
                        self.transfer
                            .run(
                                &task.id,
                                RunOptions {
                                    byte_budget: None,
                                    control: Arc::new(gate.clone()),
                                },
                            )
                            .await?,
                    );
                }
                if !raw
                    .as_ref()
                    .is_some_and(|r| r.status == TransferStatus::Complete)
                {
                    if task.origin.is_cloud()
                        && let Some(existing) = &raw
                    {
                        // A cloud original usually has no MD5. Reauthorizing on
                        // every resume would discard its valid partial bytes.
                        // Reuse the persisted URL/validators until HTTP rejects
                        // it; the existing 401/403/404/410 path then renews once.
                        self.resume_cloud(&mut task, existing, gate)?;
                    } else if task.origin.is_mv()
                        && let Some(existing) = &raw
                    {
                        // MV responses commonly omit MD5 too. Continue the same
                        // authorized representation; renew once on HTTP expiry.
                        self.resume_mv(&mut task, existing, gate)?;
                    } else {
                        self.authorize(&mut task, gate).await?;
                    }
                    for attempt in 0..2 {
                        gate.require()?;
                        match self
                            .transfer
                            .run(
                                &task.id,
                                RunOptions {
                                    byte_budget,
                                    control: Arc::new(gate.clone()),
                                },
                            )
                            .await
                        {
                            Ok(value) => {
                                raw = Some(value);
                                break;
                            }
                            Err(Error::HttpStatus {
                                status: 401 | 403 | 404 | 410,
                                ..
                            }) if attempt == 0 => self.authorize(&mut task, gate).await?,
                            Err(e) => return Err(e),
                        }
                    }
                }
                let raw = raw.ok_or(Error::State("transfer produced no result"))?;
                task.bytes = raw.bytes;
                self.save_current(&gate.token, &task)?;
                gate.require()?;
                if raw.status != TransferStatus::Complete {
                    task.status = DownloadStatus::Paused;
                    self.save_current(&gate.token, &task)?;
                    return Ok(task);
                }
                if task.raw_integrity.is_none() {
                    task.raw_integrity = raw.integrity;
                    task.phase = DownloadPhase::Prepare;
                    self.save_current(&gate.token, &task)?;
                }
                task = self.prepare(task, gate.clone()).await?;
            }
            task = self.publish(task, gate.clone()).await?;
        }
        task = self.lyrics(task, gate).await?;
        self.guard(&gate.token)?;
        gate.require_alive()?;
        self.cleanup(&gate.token, &mut task)?;
        Ok(task)
    }
    /// Explicitly discard a failed partial representation before requeueing.
    /// A prepared/published file cannot be reset by this operation.
    pub fn restart_transfer(&self, id: &str) -> Result<DownloadTask> {
        let token = self.session.token()?;
        self.guard(&token)?;
        let _lock = self.lock(id)?;
        let mut task = self.read(id)?;
        if !matches!(
            task.phase,
            DownloadPhase::Authorize | DownloadPhase::Transfer
        ) || matches!(
            task.status,
            DownloadStatus::Complete | DownloadStatus::Cancelling | DownloadStatus::Cancelled
        ) {
            return Err(Error::State("download is past raw transfer"));
        }
        self.transfer.restart(id)?;
        task.bytes = 0;
        task.raw_integrity = None;
        task.status = DownloadStatus::Queued;
        task.last_error = None;
        task.service_code = None;
        self.session.with_token(&token, |_| {
            self.store.transaction(|tx| {
                tx.delete(&self.owner, CONTROL, id)?;
                tx.put(
                    &self.owner,
                    super::manager::KIND,
                    id,
                    &serde_json::to_value(&task)?,
                )
            })
        })?;
        Ok(task)
    }
}
