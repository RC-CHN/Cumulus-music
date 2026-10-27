use super::{DownloadAction, DownloadService, manager::CONTROL};
use crate::download::directory::gate as directory_gate;
use crate::{
    Error, Result,
    account::SessionToken,
    download::transfer::{TransferControl, TransferDirective},
    storage::{OperationLock, StateTransaction},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub(super) struct Control {
    pub service: DownloadService,
    pub token: SessionToken,
    pub id: String,
    pub alive: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
    // A blocking worker retains the locks after its async caller is dropped.
    pub _task_lock: Arc<OperationLock>,
    pub _scheduler_lock: Option<Arc<OperationLock>>,
}
impl Control {
    pub fn read_tx(
        tx: &StateTransaction<'_>,
        owner: &str,
        id: &str,
    ) -> Result<Option<DownloadAction>> {
        tx.get(owner, CONTROL, id)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }
    pub fn action(&self) -> Result<Option<DownloadAction>> {
        self.service
            .store
            .get(&self.service.owner, CONTROL, &self.id)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }
    pub fn require(&self) -> Result<()> {
        if self.checkpoint()? == TransferDirective::Pause {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    pub fn require_alive(&self) -> Result<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    /// Used inside the publication transaction; never re-enter SQLite here.
    pub fn transaction_gate(&self, tx: &StateTransaction<'_>, reconciling: bool) -> Result<()> {
        self.require_alive()?;
        if !reconciling
            && (self.stop.load(Ordering::SeqCst)
                || Self::read_tx(tx, &self.service.owner, &self.id)?.is_some()
                || tx
                    .get(&self.service.owner, directory_gate::SETTINGS, "active")?
                    .is_some())
        {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
}
impl TransferControl for Control {
    fn checkpoint(&self) -> Result<TransferDirective> {
        self.service.guard(&self.token)?;
        self.require_alive()?;
        Ok(
            if self.stop.load(Ordering::SeqCst)
                || self.action()?.is_some()
                || directory_gate::active(&self.service.store, &self.service.owner)?.is_some()
            {
                TransferDirective::Pause
            } else {
                TransferDirective::Continue
            },
        )
    }
}
pub(super) struct Lifetime(pub Arc<AtomicBool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
