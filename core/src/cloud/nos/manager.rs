use super::*;
use crate::{
    account::{AccountSession, SessionToken},
    cloud::source::Source,
    storage::{OperationLock, StateStore},
};
use std::{path::Path, sync::Arc};
pub(super) const JOBS: &str = "nos_uploads_v1";
const CONTROL: &str = "nos_upload_controls_v1";
#[derive(Clone)]
pub struct NosUploads {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
    pub(super) backend: Arc<dyn NosBackend>,
    chunk_size: usize,
}
impl NosUploads {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_backend(session, Arc::new(ReqwestNosBackend::new()?), 256 * 1024)
    }
    pub fn with_backend(
        session: Arc<AccountSession>,
        backend: Arc<dyn NosBackend>,
        chunk_size: usize,
    ) -> Result<Self> {
        if !(1..=4 * 1024 * 1024).contains(&chunk_size) {
            return Err(Error::InvalidInput("NOS chunk size must be 1..4 MiB"));
        }
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        Ok(Self {
            store: session.persistent_store()?,
            session,
            token,
            owner,
            backend,
            chunk_size,
        })
    }
    pub(super) fn guard(&self) -> Result<()> {
        self.session.with_token(&self.token, |_| Ok(()))
    }
    pub(super) fn lock(&self, id: &str) -> Result<OperationLock> {
        self.guard()?;
        types::valid_id(id)?;
        OperationLock::acquire(
            self.store.directory(),
            "nos-upload",
            &serde_json::to_string(&(&self.owner, id))?,
        )
    }
    pub(super) fn read(&self, id: &str) -> Result<Job> {
        types::valid_id(id)?;
        let job: Job = self.session.with_token(&self.token, |_| {
            serde_json::from_value(
                self.store
                    .get(&self.owner, JOBS, id)?
                    .ok_or(Error::State("NOS task not found for account"))?,
            )
            .map_err(Into::into)
        })?;
        if job.id != id {
            return Err(Error::State("NOS task identity mismatch"));
        }
        job.validate()?;
        Ok(job)
    }
    pub(super) fn save(&self, job: &Job) -> Result<()> {
        job.validate()?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                tx.put(&self.owner, JOBS, &job.id, &serde_json::to_value(job)?)?;
                // A final accepted reply takes precedence over a stop request
                // that arrived while this last chunk was already in flight.
                if job.status == NosStatus::Uploaded {
                    tx.delete(&self.owner, CONTROL, &job.id)?;
                }
                Ok(())
            })
        })
    }
    /// Linearize a stop request against the next remote write intent. Once
    /// persisted, that chunk is in flight and its reply must still be recorded.
    pub(super) fn prepare_chunk(&self, job: &mut Job, pending: Pending) -> Result<bool> {
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let control: Option<NosControl> = tx
                    .get(&self.owner, CONTROL, &job.id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if let Some(control) = control {
                    job.status = match control {
                        NosControl::Pause => NosStatus::Paused,
                        NosControl::Cancel => NosStatus::Cancelled,
                    };
                    job.pending = None;
                } else {
                    job.pending = Some(pending);
                }
                job.validate()?;
                tx.put(&self.owner, JOBS, &job.id, &serde_json::to_value(&*job)?)?;
                Ok(control.is_none())
            })
        })
    }
    pub(super) fn requested(&self, id: &str) -> Result<Option<NosControl>> {
        self.session.with_token(&self.token, |_| {
            self.store
                .get(&self.owner, CONTROL, id)?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    pub(super) fn view(&self, job: &Job) -> Result<NosView> {
        Ok(job.view(self.requested(&job.id)?))
    }
    pub fn get(&self, id: &str) -> Result<NosView> {
        self.view(&self.read(id)?)
    }
    /// Native parent binding must refer to the exact confirmed object/input,
    /// not merely to a child with an Uploaded status and the same task ID.
    pub(crate) fn confirm_object(
        &self,
        id: &str,
        allocation: &NosAllocation,
        md5: &str,
    ) -> Result<()> {
        let job = self.read(id)?;
        if job.status != NosStatus::Uploaded
            || !job.allocation.same_object(allocation)
            || job.source.md5 != md5
        {
            return Err(Error::State("confirmed NOS object differs from parent"));
        }
        Ok(())
    }
    pub fn find(&self, id: &str) -> Result<Option<NosView>> {
        types::valid_id(id)?;
        let exists = self.session.with_token(&self.token, |_| {
            Ok(self.store.get(&self.owner, JOBS, id)?.is_some())
        })?;
        if exists {
            self.get(id).map(Some)
        } else {
            Ok(None)
        }
    }
    pub fn list(&self) -> Result<Vec<NosView>> {
        self.guard()?;
        let ids = self
            .store
            .values(&self.owner, JOBS)?
            .into_keys()
            .collect::<Vec<_>>();
        ids.iter().map(|id| self.get(id)).collect()
    }
    pub async fn discover(&self, bucket: &str) -> Result<NosNode> {
        self.guard()?;
        let result = self.backend.discover(bucket).await;
        self.guard()?;
        result
    }
    /// A caller may persist a chosen ID in its parent journal before creating
    /// the child. Repeated creation verifies identity and never resets progress.
    pub async fn create(
        &self,
        id: Option<&str>,
        source: &Path,
        allocation: NosAllocation,
        node: NosNode,
        expected_md5: &str,
        content_type: &str,
    ) -> Result<NosView> {
        self.guard()?;
        allocation.validate()?;
        types::valid_mime(content_type)?;
        super::super::api::validate_digest(expected_md5)?;
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let _lock = self.lock(&id)?;
        let path = source.to_path_buf();
        let source = tokio::task::spawn_blocking(move || Source::inspect(&path))
            .await
            .map_err(|_| Error::State("upload source worker stopped"))??;
        self.guard()?;
        if !source.md5.eq_ignore_ascii_case(expected_md5) {
            return Err(Error::Media("upload source differs from checked input"));
        }
        if self.store.get(&self.owner, JOBS, &id)?.is_some() {
            let old = self.read(&id)?;
            if old.source != source
                || !old.allocation.same_object(&allocation)
                || old.content_type != content_type
            {
                return Err(Error::State(
                    "NOS task ID belongs to another input or object",
                ));
            }
            return self.view(&old);
        }
        let job = Job {
            id,
            source,
            allocation,
            node,
            content_type: content_type.into(),
            chunk_size: self.chunk_size,
            status: NosStatus::Queued,
            offset: 0,
            context: String::new(),
            pending: None,
            renewal: None,
            error: None,
        };
        self.save(&job)?;
        self.view(&job)
    }
    pub fn control(&self, id: &str, action: NosControl) -> Result<NosView> {
        types::valid_id(id)?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let job: Job = serde_json::from_value(
                    tx.get(&self.owner, JOBS, id)?
                        .ok_or(Error::State("NOS task not found for account"))?,
                )?;
                job.validate()?;
                if job.id != id {
                    return Err(Error::State("NOS task identity mismatch"));
                }
                if matches!(job.status, NosStatus::Uploaded | NosStatus::Cancelled) {
                    return Err(Error::State("NOS transfer is already terminal"));
                }
                let old: Option<NosControl> = tx
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if old == Some(NosControl::Cancel) && action == NosControl::Pause {
                    return Err(Error::State("cancellation already requested"));
                }
                tx.put(&self.owner, CONTROL, id, &serde_json::to_value(action)?)
            })
        })?;
        match self.lock(id) {
            Ok(_lock) => {
                let mut job = self.read(id)?;
                self.apply_control(&mut job)?;
                self.view(&job)
            }
            Err(Error::MutationBusy) => self.get(id),
            Err(error) => Err(error),
        }
    }
    pub(super) fn apply_control(&self, job: &mut Job) -> Result<bool> {
        if job.status == NosStatus::Uploaded {
            return Ok(false);
        }
        let Some(control) = self.requested(&job.id)? else {
            return Ok(false);
        };
        job.status = match control {
            NosControl::Pause => NosStatus::Paused,
            NosControl::Cancel => NosStatus::Cancelled,
        };
        self.save(job)?;
        Ok(true)
    }
    pub fn resume(&self, id: &str) -> Result<NosView> {
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if matches!(job.status, NosStatus::Uploaded | NosStatus::Cancelled)
            || self.requested(id)? == Some(NosControl::Cancel)
        {
            return Err(Error::State("NOS transfer cannot resume"));
        }
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                tx.delete(&self.owner, CONTROL, id)?;
                job.status = NosStatus::Queued;
                tx.put(&self.owner, JOBS, id, &serde_json::to_value(&job)?)
            })
        })?;
        self.view(&job)
    }
    /// Same object only; new allocations cannot replace a task's bucket/key.
    pub fn replace_token(&self, id: &str, token: &str) -> Result<NosView> {
        types::valid_token(token)?;
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if matches!(job.status, NosStatus::Uploaded | NosStatus::Cancelled) {
            return Err(Error::State("NOS transfer no longer needs a token"));
        }
        job.allocation.token = token.into();
        job.renewal = Some(Renewal {
            status: NosRenewal::Ready,
            attempts: job.renewal.as_ref().map_or(0, |r| r.attempts),
        });
        self.save(&job)?;
        self.view(&job)
    }
    pub async fn renew_token(&self, id: &str) -> Result<NosView> {
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if matches!(job.status, NosStatus::Uploaded | NosStatus::Cancelled) {
            return Err(Error::State("NOS transfer no longer needs a token"));
        }
        self.renew(&mut job).await?;
        self.view(&job)
    }
}
