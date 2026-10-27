use super::{
    CreateTransfer, MediaBackend, ReqwestMediaBackend, TransferJob, TransferResource,
    TransferStatus as Status, files,
};
use crate::{
    Error, Result,
    storage::{OperationLock, StateStore},
};
use std::{fs, sync::Arc, time::Duration};
const KIND: &str = "media_transfers_v1";
pub struct TransferManager {
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
    pub(super) backend: Arc<dyn MediaBackend>,
    pub(super) limit: u64,
}
impl TransferManager {
    pub fn new(store: Arc<StateStore>, owner: String) -> Result<Self> {
        Self::with_backend(
            store,
            owner,
            Arc::new(ReqwestMediaBackend::new(Duration::from_secs(30))?),
            512 * 1024 * 1024,
        )
    }
    pub fn with_backend(
        store: Arc<StateStore>,
        owner: String,
        backend: Arc<dyn MediaBackend>,
        limit: u64,
    ) -> Result<Self> {
        if owner.is_empty() || owner.len() > 256 || limit == 0 {
            return Err(Error::InvalidInput("invalid transfer owner or size limit"));
        }
        Ok(Self {
            store,
            owner,
            backend,
            limit,
        })
    }
    pub fn create(&self, mut input: CreateTransfer) -> Result<TransferJob> {
        input.resource.validate(self.limit)?;
        let id = input
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let _lock = self.lock(&id)?;
        crate::download::directory::gate::require_transfer(&self.store, &self.owner, &id)?;
        if self.store.get(&self.owner, KIND, &id)?.is_some() {
            return Err(Error::State("transfer ID already exists"));
        }
        let target = files::target_path(&input.target)?;
        let parent = target.parent().expect("normalized target");
        let part = parent.join(format!(".cumulus-{id}.part"));
        let parent_token = files::token(parent)?;
        let file = files::create_part(&part)?;
        let prepared = (|| {
            file.sync_all()?;
            let part_token = files::token(&part)?;
            if !files::matches_handle(&file, &part)? {
                return Err(Error::State("new transfer staging file changed"));
            }
            let job = TransferJob {
                id,
                resource: input.resource,
                resource_id: input.resource_id,
                target,
                part: part.clone(),
                status: Status::Queued,
                bytes: 0,
                etag: None,
                modified: None,
                integrity: None,
                cleanup_pending: false,
                representation_restarted: false,
                last_error: None,
                parent_token,
                part_token,
            };
            files::sync_parent(&job)?;
            self.save(&job)?;
            Ok(job)
        })();
        if prepared.is_err() && files::matches_handle(&file, &part).unwrap_or(false) {
            let _ = fs::remove_file(&part);
        }
        prepared
    }
    pub fn get(&self, id: &str) -> Result<TransferJob> {
        validate_id(id)?;
        let job: TransferJob = serde_json::from_value(
            self.store
                .get(&self.owner, KIND, id)?
                .ok_or(Error::State("transfer does not belong to this owner"))?,
        )?;
        if job.id != id {
            return Err(Error::State("stored transfer ID mismatch"));
        }
        Ok(job)
    }
    pub fn list(&self) -> Result<Vec<TransferJob>> {
        self.store
            .values(&self.owner, KIND)?
            .into_values()
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .collect()
    }
    pub(super) fn save(&self, job: &TransferJob) -> Result<()> {
        self.store
            .put(&self.owner, KIND, &job.id, &serde_json::to_value(job)?)
    }
    pub(super) fn lock(&self, id: &str) -> Result<OperationLock> {
        validate_id(id)?;
        OperationLock::acquire(
            self.store.directory(),
            "media-transfer",
            &serde_json::to_string(&(&self.owner, id))?,
        )
    }
    /// Explicit authorization renewal. Preserve bytes only when an identical
    /// nonempty MD5 and total length prove the new response is the same media.
    pub fn refresh_resource(
        &self,
        id: &str,
        mut resource: TransferResource,
    ) -> Result<TransferJob> {
        resource.validate(self.limit)?;
        let _lock = self.lock(id)?;
        crate::download::directory::gate::require_transfer(&self.store, &self.owner, id)?;
        let mut job = self.get(id)?;
        mutable(&job)?;
        files::ensure_unpublished(&job)?;
        let same = resource.md5.is_some()
            && resource.md5 == job.resource.md5
            && resource.size == job.resource.size;
        if !same {
            let file = files::open_part(&job)?;
            file.set_len(0)?;
            file.sync_all()?;
            job.bytes = 0;
        }
        job.resource = resource;
        job.status = Status::Queued;
        job.etag = None;
        job.modified = None;
        job.last_error = None;
        job.integrity = None;
        job.representation_restarted = !same;
        self.save(&job)?;
        Ok(job)
    }
    pub fn restart(&self, id: &str) -> Result<TransferJob> {
        let _lock = self.lock(id)?;
        crate::download::directory::gate::require_transfer(&self.store, &self.owner, id)?;
        let mut job = self.get(id)?;
        mutable(&job)?;
        files::ensure_unpublished(&job)?;
        let file = files::open_part(&job)?;
        file.set_len(0)?;
        file.sync_all()?;
        job.bytes = 0;
        job.etag = None;
        job.modified = None;
        job.integrity = None;
        job.status = Status::Queued;
        job.last_error = None;
        self.save(&job)?;
        Ok(job)
    }
    /// Does not delete a completed output or adopt/remove a foreign file.
    pub fn cancel(&self, id: &str) -> Result<TransferJob> {
        let _lock = self.lock(id)?;
        crate::download::directory::gate::require_transfer(&self.store, &self.owner, id)?;
        let mut job = self.get(id)?;
        if job.status == Status::Complete {
            return Err(Error::State("completed transfer cannot be cancelled"));
        }
        if job.status == Status::Cancelled {
            return Ok(job);
        }
        files::ensure_unpublished(&job)?;
        job.status = Status::Cancelling;
        self.save(&job)?;
        files::remove_part(&job)?;
        job.status = Status::Cancelled;
        job.bytes = 0;
        self.save(&job)?;
        Ok(job)
    }
    /// Only the higher-level task may discard its own intermediate output.
    /// This is intentionally not part of the public completed-file API.
    pub(crate) fn discard_intermediate(&self, id: &str, expected: &std::path::Path) -> Result<()> {
        let _lock = self.lock(id)?;
        let mut job = self.get(id)?;
        if job.target != expected {
            return Err(Error::State("unexpected intermediate transfer path"));
        }
        let owned = files::target_is_owned(&job)?;
        if !owned {
            files::absent(&job.target)?;
        }
        job.status = Status::Cancelling;
        self.save(&job)?;
        if owned {
            fs::remove_file(&job.target)?;
            files::sync_parent(&job)?;
        }
        files::remove_part(&job)?;
        job.status = Status::Cancelled;
        job.bytes = 0;
        self.save(&job)
    }
}
fn mutable(job: &TransferJob) -> Result<()> {
    if matches!(
        job.status,
        Status::Complete | Status::Cancelled | Status::Cancelling | Status::Publishing
    ) {
        return Err(Error::State(
            "terminal or publishing transfer cannot be reset",
        ));
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid transfer ID"));
    }
    Ok(())
}

pub(super) struct RunMarker<'a> {
    pub manager: &'a TransferManager,
    pub id: &'a str,
    pub armed: bool,
}
impl Drop for RunMarker<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Ok(mut job) = self.manager.get(self.id)
            && job.status == Status::Running
        {
            if let Ok(file) = files::open_part(&job) {
                let _ = file.sync_all();
                if let Ok(meta) = file.metadata() {
                    job.bytes = meta.len();
                }
            }
            job.status = Status::Paused;
            job.last_error = Some("worker_interrupted".into());
            let _ = self.manager.save(&job);
        }
    }
}
