use super::*;
use crate::{
    account::{AccountSession, SessionToken},
    cloud::source::Source,
    storage::{OperationLock, StateStore},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};
pub(super) const JOBS: &str = "oss_uploads_v1";
pub(super) const CONTROL: &str = "oss_upload_controls_v1";
#[derive(Clone)]
pub struct OssUploads {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
    pub(super) backend: Arc<dyn OssBackend>,
    part_size: usize,
    concurrency: usize,
}
impl OssUploads {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_backend(session, Arc::new(ReqwestOssBackend::new()?), 1024 * 1024, 3)
    }
    pub fn with_backend(
        session: Arc<AccountSession>,
        backend: Arc<dyn OssBackend>,
        part_size: usize,
        concurrency: usize,
    ) -> Result<Self> {
        state::configuration(part_size, concurrency)?;
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        Ok(Self {
            store: session.persistent_store()?,
            session,
            token,
            owner,
            backend,
            part_size,
            concurrency,
        })
    }
    pub(super) fn guard(&self) -> Result<()> {
        self.session.with_token(&self.token, |_| Ok(()))
    }
    pub(super) fn lock(&self, id: &str) -> Result<OperationLock> {
        self.guard()?;
        state::valid_id(id)?;
        OperationLock::acquire(
            self.store.directory(),
            "oss-upload",
            &serde_json::to_string(&(&self.owner, id))?,
        )
    }
    pub(super) fn read(&self, id: &str) -> Result<Job> {
        state::valid_id(id)?;
        let job: Job = self.session.with_token(&self.token, |_| {
            Ok(serde_json::from_value(
                self.store
                    .get(&self.owner, JOBS, id)?
                    .ok_or(Error::State("OSS task not found for account"))?,
            )?)
        })?;
        job.validate()?;
        if job.id != id {
            return Err(Error::State("OSS task identity mismatch"));
        }
        Ok(job)
    }
    pub(super) fn save(&self, job: &Job) -> Result<()> {
        job.validate()?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                tx.put(&self.owner, JOBS, &job.id, &serde_json::to_value(job)?)?;
                if matches!(job.status, OssStatus::Uploaded | OssStatus::Cancelled) {
                    tx.delete(&self.owner, CONTROL, &job.id)?;
                }
                Ok(())
            })
        })
    }
    pub(super) fn requested(&self, id: &str) -> Result<Option<OssControl>> {
        self.session.with_token(&self.token, |_| {
            self.store
                .get(&self.owner, CONTROL, id)?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    pub(super) fn view(&self, job: &Job) -> Result<OssView> {
        Ok(job.view(self.requested(&job.id)?))
    }
    pub fn get(&self, id: &str) -> Result<OssView> {
        self.view(&self.read(id)?)
    }
    pub fn find(&self, id: &str) -> Result<Option<OssView>> {
        self.guard()?;
        state::valid_id(id)?;
        let exists = self.session.with_token(&self.token, |_| {
            Ok(self.store.get(&self.owner, JOBS, id)?.is_some())
        })?;
        if exists {
            self.get(id).map(Some)
        } else {
            Ok(None)
        }
    }
    pub fn list(&self) -> Result<Vec<OssView>> {
        let ids = self.session.with_token(&self.token, |_| {
            Ok(self
                .store
                .values(&self.owner, JOBS)?
                .into_keys()
                .collect::<Vec<_>>())
        })?;
        ids.iter().map(|id| self.get(id)).collect()
    }
    pub async fn create(
        &self,
        id: Option<&str>,
        path: &Path,
        allocation: OssAllocation,
        expected_md5: &str,
    ) -> Result<OssView> {
        self.guard()?;
        allocation.validate()?;
        super::super::api::validate_digest(expected_md5)?;
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let _lock = self.lock(&id)?;
        let path = path.to_owned();
        let (source, crc) = tokio::task::spawn_blocking(move || {
            let source = Source::inspect(&path)?;
            let reader = source.open()?;
            let mut file = reader.lock().map_err(|_| Error::LockPoisoned)?;
            file.seek(SeekFrom::Start(0))?;
            let mut buffer = vec![0; 65536];
            let mut crc = 0;
            let mut bytes = 0;
            loop {
                let n = match file.read(&mut buffer) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    v => v?,
                };
                if n == 0 {
                    break;
                }
                bytes += n as u64;
                if bytes > source.bytes {
                    return Err(Error::Media("OSS input grew while hashing"));
                }
                crc = crc64(&buffer[..n], crc);
            }
            source.verify(&mut file)?;
            if Source::inspect(&source.path)? != source {
                return Err(Error::Media("OSS input changed while preparing"));
            }
            Ok((source, crc))
        })
        .await
        .map_err(|_| Error::State("OSS source worker stopped"))??;
        self.guard()?;
        if !source.md5.eq_ignore_ascii_case(expected_md5) {
            return Err(Error::Media("OSS source differs from checked audio"));
        }
        if self.find(&id)?.is_some() {
            let old = self.read(&id)?;
            if old.source != source || !old.allocation.same_object(&allocation) {
                return Err(Error::State(
                    "OSS child ID belongs to another input or object",
                ));
            }
            return self.view(&old);
        }
        let job = Job {
            id,
            source,
            allocation,
            crc64: crc,
            part_size: self.part_size,
            concurrency: self.concurrency,
            status: OssStatus::Queued,
            upload_id: None,
            parts: BTreeMap::new(),
            pending: None,
            completion: None,
            error: None,
        };
        self.save(&job)?;
        self.view(&job)
    }
    pub fn replace_credentials(&self, id: &str, allocation: OssAllocation) -> Result<OssView> {
        allocation.validate()?;
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if matches!(job.status, OssStatus::Uploaded | OssStatus::Cancelled) {
            return Err(Error::State("OSS task is terminal"));
        }
        if !job.allocation.same_object(&allocation) {
            return Err(Error::InvalidInput("OSS renewal changed object scope"));
        }
        job.allocation = allocation;
        self.save(&job)?;
        self.view(&job)
    }
    /// Durable request, cooperative at batch boundaries. Cancellation is only
    /// complete after run/cancel has acknowledged its known multipart abort.
    pub fn control(&self, id: &str, action: OssControl) -> Result<OssView> {
        state::valid_id(id)?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let job: Job = serde_json::from_value(
                    tx.get(&self.owner, JOBS, id)?
                        .ok_or(Error::State("OSS task not found for account"))?,
                )?;
                job.validate()?;
                if job.id != id || matches!(job.status, OssStatus::Uploaded | OssStatus::Cancelled)
                {
                    return Err(Error::State("OSS task is terminal or mismatched"));
                }
                let old: Option<OssControl> = tx
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if old == Some(OssControl::Cancel) && action == OssControl::Pause {
                    return Err(Error::State("OSS cancellation already requested"));
                }
                tx.put(&self.owner, CONTROL, id, &serde_json::to_value(action)?)
            })
        })?;
        self.get(id)
    }
    pub fn resume(&self, id: &str) -> Result<OssView> {
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if matches!(job.status, OssStatus::Uploaded | OssStatus::Cancelled) {
            return Err(Error::State("OSS task cannot resume"));
        }
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let requested: Option<OssControl> = tx
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if requested == Some(OssControl::Cancel) {
                    return Err(Error::State("OSS cancellation already requested"));
                }
                tx.delete(&self.owner, CONTROL, id)?;
                job.status = OssStatus::Queued;
                tx.put(&self.owner, JOBS, id, &serde_json::to_value(&job)?)
            })
        })?;
        self.view(&job)
    }
    pub(super) fn begin(&self, job: &mut Job, pending: Pending) -> Result<()> {
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, CONTROL, &job.id)?.is_some() {
                    return Err(Error::Cancelled);
                }
                job.pending = Some(pending);
                job.validate()?;
                tx.put(&self.owner, JOBS, &job.id, &serde_json::to_value(&*job)?)
            })
        })
    }
    pub(super) async fn call(&self, job: &Job, operation: OssOperation) -> Result<OssReply> {
        self.guard()?;
        let request = OssRequest::new(&job.allocation, &job.id, operation, &http_date())?;
        let result = self.backend.request(request).await;
        self.guard()?;
        result
    }
}
fn http_date() -> String {
    let now = time::OffsetDateTime::now_utc();
    let week = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        [now.weekday().number_days_from_monday() as usize];
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][now.month() as usize - 1];
    format!(
        "{week}, {:02} {month} {:04} {:02}:{:02}:{:02} GMT",
        now.day(),
        now.year(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
