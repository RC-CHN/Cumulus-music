use super::{DownloadRunOptions, DownloadService, DownloadStatus};
use crate::{Error, Result, storage::OperationLock};
use serde::Serialize;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::task::JoinSet;

pub struct DownloadScheduler {
    service: DownloadService,
    concurrency: usize,
    stop: Arc<AtomicBool>,
}
#[derive(Debug, Serialize)]
pub struct ScheduledResult {
    pub id: String,
    pub status: DownloadStatus,
    pub error: Option<String>,
}
struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl DownloadScheduler {
    pub fn new(service: DownloadService, concurrency: usize) -> Result<Self> {
        if !(1..=3).contains(&concurrency) {
            return Err(Error::InvalidInput("download concurrency must be 1..3"));
        }
        Ok(Self {
            service,
            concurrency,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    /// Finite drain. Each task is attempted once; paused/errors need enqueue.
    /// A slot includes authorization, media, tags, publication and sidecars.
    pub async fn run_pending(&self) -> Result<Vec<ScheduledResult>> {
        self.drain(None).await
    }
    /// Drain only explicitly selected tasks, preserving the same account-wide
    /// scheduler lock and full-task concurrency bound.
    pub async fn run_selected(&self, ids: &[String]) -> Result<Vec<ScheduledResult>> {
        for id in ids {
            self.service.get(id)?;
        }
        self.drain(Some(ids.iter().cloned().collect())).await
    }
    async fn drain(&self, selected: Option<HashSet<String>>) -> Result<Vec<ScheduledResult>> {
        let token = self.service.session.token()?;
        self.service.guard(&token)?;
        crate::download::directory::gate::require(&self.service.store, &self.service.owner, None)?;
        let lock = Arc::new(OperationLock::acquire(
            self.service.store.directory(),
            "download-scheduler",
            &self.service.owner,
        )?);
        self.stop.store(false, Ordering::SeqCst);
        let _stop = StopOnDrop(self.stop.clone());
        let mut active = JoinSet::new();
        let mut attempted = HashSet::new();
        let mut results = Vec::new();
        loop {
            if self.service.guard(&token).is_err()
                || crate::download::directory::gate::active(
                    &self.service.store,
                    &self.service.owner,
                )?
                .is_some()
            {
                self.stop();
            }
            if !self.stop.load(Ordering::SeqCst) {
                for task in self
                    .service
                    .list()?
                    .into_iter()
                    .filter(|t| selected.as_ref().is_none_or(|ids| ids.contains(&t.id)))
                    .filter(|t| {
                        matches!(
                            t.status,
                            DownloadStatus::Queued
                                | DownloadStatus::Authorizing
                                | DownloadStatus::Downloading
                                | DownloadStatus::Processing
                                | DownloadStatus::Cancelling
                        )
                    })
                    .filter(|t| !attempted.contains(&t.id))
                    .take(self.concurrency - active.len())
                    .collect::<Vec<_>>()
                {
                    attempted.insert(task.id.clone());
                    let service = self.service.clone();
                    let stop = self.stop.clone();
                    let lock = lock.clone();
                    let token = token.clone();
                    active.spawn(async move {
                        let result = service
                            .run_locked(
                                &task.id,
                                DownloadRunOptions {
                                    byte_budget: None,
                                    stop,
                                },
                                Some(lock),
                                token,
                            )
                            .await;
                        let status = service
                            .read(&task.id)
                            .map(|v| v.status)
                            .unwrap_or(DownloadStatus::Error);
                        ScheduledResult {
                            id: task.id,
                            status,
                            error: result.err().map(|e| e.to_string()),
                        }
                    });
                }
            }
            if active.is_empty() {
                break;
            }
            let result = active
                .join_next()
                .await
                .ok_or(Error::State("download scheduler lost a worker"))?
                .map_err(|_| Error::State("download worker stopped unexpectedly"))?;
            results.push(result);
        }
        Ok(results)
    }
}
