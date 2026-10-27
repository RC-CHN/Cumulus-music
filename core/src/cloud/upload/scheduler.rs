use super::*;
use crate::storage::OperationLock;
use serde::{Deserialize, Serialize};
const SCHEDULE: &str = "cloud_schedule_v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadSchedule {
    pub id: String,
    pub order: u64,
    pub revision: u64,
    pub enabled: bool,
    pub running: bool,
    pub hidden: bool,
    pub retry_rejected: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct UploadScheduler {
    uploads: CloudUploads,
}
impl UploadScheduler {
    pub fn new(uploads: CloudUploads) -> Self {
        Self { uploads }
    }
    fn lock(&self) -> Result<OperationLock> {
        self.uploads.guard()?;
        OperationLock::acquire(
            self.uploads.store.directory(),
            "cloud-scheduler",
            &self.uploads.owner,
        )
    }
    pub fn entries(&self) -> Result<Vec<UploadSchedule>> {
        self.uploads.session.with_token(&self.uploads.token, |_| {
            let mut values = self
                .uploads
                .store
                .values(&self.uploads.owner, SCHEDULE)?
                .into_iter()
                .map(|(key, v)| {
                    let entry: UploadSchedule = serde_json::from_value(v)?;
                    types::valid_id(&entry.id)?;
                    if key != entry.id {
                        return Err(Error::State("cloud schedule identity mismatch"));
                    }
                    Ok(entry)
                })
                .collect::<Result<Vec<_>>>()?;
            values.sort_by_key(|v| (v.order, v.id.clone()));
            Ok(values)
        })
    }
    fn change(
        &self,
        id: &str,
        change: impl FnOnce(&mut UploadSchedule) -> Result<()>,
    ) -> Result<UploadSchedule> {
        self.uploads.get(id)?;
        self.uploads.session.with_token(&self.uploads.token, |_| {
            self.uploads.store.transaction(|tx| {
                let mut state = match tx.get(&self.uploads.owner, SCHEDULE, id)? {
                    Some(value) => serde_json::from_value(value)?,
                    None => UploadSchedule {
                        id: id.into(),
                        order: self.uploads.clock.now_ms()?,
                        revision: 0,
                        enabled: false,
                        running: false,
                        hidden: false,
                        retry_rejected: false,
                        error: None,
                    },
                };
                if state.id != id {
                    return Err(Error::State("cloud schedule identity mismatch"));
                }
                change(&mut state)?;
                tx.put(
                    &self.uploads.owner,
                    SCHEDULE,
                    id,
                    &serde_json::to_value(&state)?,
                )?;
                Ok(state)
            })
        })
    }
    /// Call once when a native host opens this account. Official M/3127 pauses
    /// unfinished transfers restored from disk; opening a page never resumes them.
    pub async fn recover(&self) -> Result<()> {
        let _lock = self.lock()?;
        for task in self.uploads.list()? {
            self.change(&task.id, |state| {
                state.enabled = false;
                state.running = false;
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("cloud schedule exhausted"))?;
                Ok(())
            })?;
            if !matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                && task.requested != Some(UploadControl::Cancel)
            {
                self.uploads.control(&task.id, UploadControl::Pause).await?;
            }
        }
        Ok(())
    }
    pub fn start(&self, id: &str, retry_rejected: bool) -> Result<UploadSchedule> {
        let task = self.uploads.get(id)?;
        if task.requested == Some(UploadControl::Pause) {
            self.uploads.resume(id)?;
        }
        if matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled)
            || task.requested == Some(UploadControl::Cancel)
        {
            return Err(Error::State("cloud task cannot start"));
        }
        if task.phase == CloudPhase::Failed && !retry_rejected {
            return Err(Error::State("cloud rejection requires explicit retry"));
        }
        if matches!(
            task.phase,
            CloudPhase::AllocationPending
                | CloudPhase::MetadataPending
                | CloudPhase::PublicationPending
        ) {
            return Err(Error::MutationUncertain);
        }
        self.change(id, |state| {
            if state.running {
                return Err(Error::MutationBusy);
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or(Error::State("cloud schedule exhausted"))?;
            state.enabled = true;
            state.hidden = false;
            state.error = None;
            state.retry_rejected = retry_rejected;
            Ok(())
        })
    }
    pub async fn control(&self, id: &str, action: UploadControl) -> Result<CloudView> {
        self.change(id, |state| {
            state.enabled = false;
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or(Error::State("cloud schedule exhausted"))?;
            Ok(())
        })?;
        self.uploads.control(id, action).await
    }
    pub fn dismiss(&self, id: &str) -> Result<()> {
        if !matches!(
            self.uploads.get(id)?.phase,
            CloudPhase::Complete | CloudPhase::Cancelled
        ) {
            return Err(Error::State("unfinished cloud task cannot be dismissed"));
        }
        self.change(id, |state| {
            state.hidden = true;
            state.enabled = false;
            Ok(())
        })?;
        Ok(())
    }
    /// One ready task and a bounded batch. Host schedules future calls; account
    /// APIs, storage and object HTTP remain entirely inside this Rust service.
    pub async fn step(&self) -> Result<Option<CloudView>> {
        let _lock = self.lock()?;
        for entry in self
            .entries()?
            .into_iter()
            .filter(|v| v.enabled && !v.hidden)
        {
            let before = self.uploads.get(&entry.id)?;
            if before.next_check_ms > self.uploads.clock.now_ms()? {
                continue;
            }
            if matches!(before.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                || before.requested.is_some()
            {
                self.change(&entry.id, |state| {
                    state.enabled = false;
                    state.running = false;
                    Ok(())
                })?;
                continue;
            }
            let dispatch = self.change(&entry.id, |state| {
                if state.revision != entry.revision || !state.enabled {
                    return Err(Error::Cancelled);
                }
                state.running = true;
                Ok(())
            });
            let dispatch = match dispatch {
                Err(Error::Cancelled) => continue,
                other => other?,
            };
            let result = self
                .uploads
                .run(
                    &entry.id,
                    CloudRunOptions {
                        chunk_budget: Some(3),
                        retry_rejected: dispatch.retry_rejected,
                    },
                )
                .await;
            self.uploads.guard()?;
            if let Err(error @ Error::Storage(_)) = result {
                return Err(error);
            }
            let current = self.uploads.get(&entry.id)?;
            self.change(&entry.id, |state| {
                state.running = false;
                if state.revision == dispatch.revision {
                    state.retry_rejected = false;
                    if let Err(error) = &result {
                        state.error = Some(
                            if current
                                .error
                                .as_ref()
                                .is_some_and(|v| v.kind == "write_outcome_unknown")
                            {
                                "write_outcome_unknown"
                            } else {
                                match error.root() {
                                    Error::MutationUncertain => "write_outcome_unknown",
                                    Error::Media(_) => "source_changed",
                                    Error::Transport(_) | Error::HttpStatus { .. } => "network",
                                    Error::Service { .. } => "service_rejected",
                                    Error::Io(_) => "file_unavailable",
                                    _ => "upload_incomplete",
                                }
                            }
                            .into(),
                        );
                        state.enabled = false;
                    }
                    if matches!(
                        current.phase,
                        CloudPhase::Complete | CloudPhase::Cancelled | CloudPhase::Failed
                    ) || current.requested.is_some()
                    {
                        state.enabled = false;
                    }
                }
                Ok(())
            })?;
            return Ok(Some(current));
        }
        Ok(None)
    }
}
