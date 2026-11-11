//! Passive desktop startup. The explicit scheduler/standalone service keeps
//! its original recovery policy; opening the download page does not reauthorize.
use super::{
    DownloadAction, DownloadPhase, DownloadService, DownloadStatus, DownloadTask, control::Control,
    files, manager::KIND, scheduler,
};
use crate::{
    Error, Result,
    account::SessionToken,
    download::{
        directory::gate as directory_gate,
        transfer::{FINAL_INTEGRITY_MISMATCH, TransferJob, TransferStatus},
    },
    storage::OperationLock,
};
use std::sync::{Arc, atomic::AtomicBool};

fn runnable(status: DownloadStatus) -> bool {
    matches!(
        status,
        DownloadStatus::Queued
            | DownloadStatus::Authorizing
            | DownloadStatus::Downloading
            | DownloadStatus::Processing
    )
}

impl DownloadService {
    pub(crate) fn restore_desktop_queue(&self, token: &SessionToken) -> Result<()> {
        self.guard(token)?;
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        let mut tasks = self.list()?;
        tasks.retain(|task| {
            !matches!(
                task.status,
                DownloadStatus::Complete | DownloadStatus::Cancelled
            )
        });
        if tasks.is_empty() {
            return self.guard(token);
        }
        let _scheduler =
            OperationLock::acquire(self.store.directory(), scheduler::LOCK, &self.owner)?;
        let migrating = directory_gate::active(&self.store, &self.owner)?.is_some();
        for task in tasks {
            let lock = Arc::new(self.lock(&task.id)?);
            let task = self.read(&task.id)?;
            if matches!(
                task.status,
                DownloadStatus::Complete | DownloadStatus::Cancelled
            ) {
                continue;
            }
            if !migrating
                && task.file_operation.is_none()
                && task.phase == DownloadPhase::Publish
                && files::published(&task).unwrap_or(false)
            {
                // If the final link already exists, adopt its verified identity
                // into the account ledger. Do not create a link, fetch lyrics or
                // clean up artifacts on this path.
                let gate = Control {
                    service: self.clone(),
                    token: token.clone(),
                    id: task.id.clone(),
                    alive: Arc::new(AtomicBool::new(true)),
                    stop: Arc::new(AtomicBool::new(true)),
                    _task_lock: lock.clone(),
                    _scheduler_lock: None,
                };
                match self.publish_audio(task.clone(), &gate, true) {
                    Ok(_) => continue,
                    Err(Error::Media(_) | Error::State(_) | Error::Io(_) | Error::Cancelled) => {
                        // A moved/unreadable/corrupt target cannot be declared
                        // complete. Preserve the task and prepared journal.
                    }
                    Err(error) => return Err(error),
                }
            }
            self.freeze_restored_task(token, task, migrating)?;
        }
        self.guard(token)
    }

    fn freeze_restored_task(
        &self,
        token: &SessionToken,
        mut task: DownloadTask,
        migrating: bool,
    ) -> Result<()> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let control = Control::read_tx(tx, owner, &task.id)?;
                let status = task.status;
                // A staged file move owns an exact before/after task snapshot.
                // Preserve that row and pass intent through the control record
                // until migration finalization releases it.
                let relocating = task.file_operation.is_some();
                if !relocating && control == Some(DownloadAction::Cancel) {
                    // Complete only a cancellation the user already committed.
                    task.status = DownloadStatus::Cancelling;
                } else if !relocating && runnable(status) {
                    let integrity_error = Error::Media(FINAL_INTEGRITY_MISMATCH).to_string();
                    let failed = if task.phase == DownloadPhase::Transfer {
                        tx.get(owner, "media_transfers_v1", &task.id)?
                            .map(serde_json::from_value::<TransferJob>)
                            .transpose()?
                            .is_some_and(|raw| {
                                raw.status == TransferStatus::Error
                                    && raw.last_error.as_deref() == Some(&integrity_error)
                            })
                    } else {
                        false
                    };
                    if failed {
                        task.status = DownloadStatus::Error;
                        task.last_error = Some(integrity_error);
                        task.service_code = None;
                    } else {
                        task.status = DownloadStatus::Paused;
                    }
                }
                // Migration finalization restores its own pre-move queue from
                // a separate journal. Fence that requeue if the app was closed
                // before migration completion. No data is moved on startup.
                if migrating
                    && (task.status == DownloadStatus::Paused || relocating && runnable(status))
                    && control.is_none()
                {
                    Control::record(tx, owner, &task.id, DownloadAction::Pause)?;
                }
                if task.status != status {
                    tx.put(owner, KIND, &task.id, &serde_json::to_value(&task)?)?;
                }
                Ok(())
            })
        })
    }
}
