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
    #[serde(default)]
    pub removing: bool,
    pub retry_rejected: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct UploadScheduler {
    pub(super) uploads: CloudUploads,
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
    pub fn entry(&self, id: &str) -> Result<Option<UploadSchedule>> {
        types::valid_id(id)?;
        self.uploads.session.with_token(&self.uploads.token, |_| {
            let entry = self
                .uploads
                .store
                .get(&self.uploads.owner, SCHEDULE, id)?
                .map(serde_json::from_value::<UploadSchedule>)
                .transpose()?;
            if entry.as_ref().is_some_and(|entry| entry.id != id) {
                return Err(Error::State("cloud schedule identity mismatch"));
            }
            Ok(entry)
        })
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
    pub(super) fn change(
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
                        removing: false,
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
            let state = self.change(&task.id, |state| {
                state.enabled = false;
                state.running = false;
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("cloud schedule exhausted"))?;
                Ok(())
            })?;
            if state.removing {
                // Removal was explicitly authorized and Cancel was already
                // persisted before removing=true. Unknown writes stay visible.
                if matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled) {
                    self.change(&task.id, |state| {
                        state.hidden = true;
                        Ok(())
                    })?;
                } else if task.requested == Some(UploadControl::Cancel) {
                    let outcome = self.uploads.control(&task.id, UploadControl::Cancel).await;
                    self.finish_removal(&task.id, outcome)?;
                }
            } else if !matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                && task.requested != Some(UploadControl::Cancel)
            {
                self.uploads.control(&task.id, UploadControl::Pause).await?;
            }
        }
        Ok(())
    }
    /// Record only a local omission, without starting or enabling any work.
    pub(crate) fn omit_text_attachment(
        &self,
        id: &str,
        kind: AttachmentKind,
        expected: &str,
    ) -> Result<()> {
        use manager::{CONTROL, JOBS};
        let _task_lock = self.uploads.lock(id)?;
        let task = self.uploads.read(id)?;
        if task.uncertain() {
            return Err(Error::MutationUncertain);
        }
        if task.omittable_text() != Some(kind)
            || self.uploads.nos.find(&task.child_id(Some(kind)))?.is_some()
        {
            return Err(Error::StaleResponse);
        }
        self.uploads.session.with_token(&self.uploads.token, |_| {
            self.uploads.store.transaction(|tx| {
                let owner = &self.uploads.owner;
                let mut current: Task =
                    serde_json::from_value(tx.get(owner, JOBS, id)?.ok_or(Error::StaleResponse)?)?;
                current.validate()?;
                let mut state: UploadSchedule = serde_json::from_value(
                    tx.get(owner, SCHEDULE, id)?.ok_or(Error::StaleResponse)?,
                )?;
                let control: Option<UploadControl> = tx
                    .get(owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if current.id != id
                    || current.omittable_text() != Some(kind)
                    || state.id != id
                    || state.revision.to_string() != expected
                    || state.enabled
                    || state.running
                    || state.hidden
                    || state.removing
                    || control == Some(UploadControl::Cancel)
                {
                    return Err(Error::StaleResponse);
                }
                let attachment = current
                    .attachments
                    .iter_mut()
                    .find(|a| a.kind == kind)
                    .ok_or(Error::StaleResponse)?;
                attachment.phase = AttachmentPhase::Omitted;
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("cloud schedule exhausted"))?;
                state.error = None;
                tx.put(owner, JOBS, id, &serde_json::to_value(current)?)?;
                tx.put(owner, SCHEDULE, id, &serde_json::to_value(state)?)
            })
        })
    }
    pub fn start(&self, id: &str, retry_rejected: bool) -> Result<UploadSchedule> {
        let _lock = self.uploads.lock(id)?;
        let record = self.uploads.read(id)?;
        let task = self.uploads.view(&record)?;
        if task.unresolved_nos_boundary() {
            return Err(Error::MutationUncertain);
        }
        if task.blocked_text() {
            return Err(Error::State(
                "blocked text attachment requires explicit omission",
            ));
        }
        if self.entry(id)?.is_some_and(|entry| entry.removing) {
            return Err(Error::State("cloud task removal is pending"));
        }
        if task.requested == Some(UploadControl::Pause) {
            self.uploads.resume_locked(&record)?;
        }
        if matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled)
            || task.requested == Some(UploadControl::Cancel)
        {
            return Err(Error::State("cloud task cannot start"));
        }
        if task.phase == CloudPhase::Failed
            && task
                .error
                .as_ref()
                .is_some_and(|error| error.kind == "encoding_failed")
        {
            return Err(Error::State("cloud encoding retry requires status check"));
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
                if state.removing
                    && matches!(current.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                {
                    state.hidden = true;
                    state.enabled = false;
                }
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
