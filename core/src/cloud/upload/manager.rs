use super::*;
use crate::{
    account::{AccountSession, SessionToken},
    library::probe::{AudioProbe, Ffprobe},
    storage::{OperationLock, StateStore},
};
use std::{future::Future, path::Path, sync::Arc};
pub(super) const JOBS: &str = "cloud_uploads_v1";
pub(super) const CONTROL: &str = "cloud_upload_controls_v1";
// Official M/318 startup default; only for admission of new local audio.
const NEW_AUDIO_LIMIT: u64 = 100 * 1024 * 1024;
#[derive(Clone)]
pub struct UploadAdapters {
    pub nos: Arc<dyn NosBackend>,
    pub oss: Arc<dyn OssBackend>,
    pub probe: Arc<dyn AudioProbe>,
    pub clock: Arc<dyn UploadClock>,
    pub chunk_size: usize,
    pub oss_part_size: usize,
    pub oss_concurrency: usize,
}
impl UploadAdapters {
    pub fn native() -> Result<Self> {
        Ok(Self {
            nos: Arc::new(ReqwestNosBackend::new()?),
            oss: Arc::new(ReqwestOssBackend::new()?),
            probe: Arc::new(Ffprobe::default()),
            clock: Arc::new(types::SystemClock),
            chunk_size: 256 * 1024,
            oss_part_size: 1024 * 1024,
            oss_concurrency: 3,
        })
    }
}
#[derive(Clone)]
pub struct CloudUploads {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
    pub(super) nos: NosUploads,
    pub(super) oss: OssUploads,
    pub(super) clock: Arc<dyn UploadClock>,
    probe: Arc<dyn AudioProbe>,
}
impl CloudUploads {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_adapters(session, UploadAdapters::native()?)
    }
    pub fn with_adapters(session: Arc<AccountSession>, adapters: UploadAdapters) -> Result<Self> {
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let nos = NosUploads::with_backend(session.clone(), adapters.nos, adapters.chunk_size)?;
        let oss = OssUploads::with_backend(
            session.clone(),
            adapters.oss,
            adapters.oss_part_size,
            adapters.oss_concurrency,
        )?;
        session.with_token(&token, |_| Ok(()))?;
        Ok(Self {
            store: session.persistent_store()?,
            session,
            token,
            owner,
            nos,
            oss,
            clock: adapters.clock,
            probe: adapters.probe,
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
            "cloud-upload",
            &serde_json::to_string(&(&self.owner, id))?,
        )
    }
    pub(super) fn read(&self, id: &str) -> Result<Task> {
        types::valid_id(id)?;
        let task: Task = self.session.with_token(&self.token, |_| {
            Ok(serde_json::from_value(
                self.store
                    .get(&self.owner, JOBS, id)?
                    .ok_or(Error::State("cloud upload not found for account"))?,
            )?)
        })?;
        task.validate()?;
        if task.id != id {
            return Err(Error::State("cloud upload task identity mismatch"));
        }
        Ok(task)
    }
    pub(super) fn save(&self, task: &Task) -> Result<()> {
        task.validate()?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                tx.put(&self.owner, JOBS, &task.id, &serde_json::to_value(task)?)?;
                if task.phase == CloudPhase::Complete {
                    tx.delete(&self.owner, CONTROL, &task.id)?;
                }
                Ok(())
            })
        })
    }
    pub(super) async fn call<T, F, Fut>(&self, operation: F) -> Result<T>
    where
        F: FnOnce(CloudApi) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.session
            .run_for(&self.token, |api, _| operation(CloudApi::new(api)))
            .await
    }
    pub(super) fn requested(&self, id: &str) -> Result<Option<UploadControl>> {
        self.session.with_token(&self.token, |_| {
            self.store
                .get(&self.owner, CONTROL, id)?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    pub(super) fn view(&self, task: &Task) -> Result<CloudView> {
        let audio = match task.allocation.as_ref().and_then(|v| v["channel"].as_u64()) {
            Some(1) => self.nos.find(&task.child_id(None))?.map(CloudTransfer::Nos),
            Some(3) => self.oss.find(&task.child_id(None))?.map(CloudTransfer::Oss),
            _ => None,
        };
        let mut attachments = Vec::new();
        let mut omittable_text = task.omittable_text();
        for a in &task.attachments {
            let child = self.nos.find(&task.child_id(Some(a.kind)))?;
            if omittable_text == Some(a.kind) && child.is_some() {
                omittable_text = None;
            }
            attachments.push(AttachmentView {
                kind: a.kind,
                source: a.source.path.clone(),
                phase: a.phase,
                bytes: child.as_ref().map_or(0, |c| c.bytes),
                total: a.source.bytes,
                unresolved_nos_boundary: child.as_ref().is_some_and(|c| c.unresolved_boundary),
                transfer: child.map(|c| c.status),
                error: if a.phase == AttachmentPhase::Omitted {
                    None
                } else {
                    a.error.clone()
                },
            });
        }
        let deduplicated = task.decision.as_ref().is_some_and(|d| !d.need_upload);
        Ok(CloudView {
            id: task.id.clone(),
            source: task.source.path.clone(),
            title: task.info.title.clone(),
            artists: task.info.artists.clone(),
            album: task.info.album.clone(),
            phase: task.phase,
            channel: task.allocation.as_ref().and_then(|v| v["channel"].as_u64()),
            bytes: audio.as_ref().map_or(
                if deduplicated { task.source.bytes } else { 0 },
                CloudTransfer::bytes,
            ),
            total: task.source.bytes,
            transfer: audio,
            song_id: task.song_id.clone(),
            next_check_ms: task.next_check_ms,
            requested: self.requested(&task.id)?,
            error: if task.uncertain() {
                Some(UploadIssue::new("write_outcome_unknown"))
            } else {
                task.error.clone()
            },
            attachments,
            omittable_text,
        })
    }
    pub fn get(&self, id: &str) -> Result<CloudView> {
        self.view(&self.read(id)?)
    }
    /// Resolve only a published source recorded by this account. The caller
    /// supplies an upload task identity, never an arbitrary filesystem path.
    pub fn verified_source_file(&self, id: &str, song_id: &str) -> Result<std::path::PathBuf> {
        let task = self.read(id)?;
        if task.song_id.as_deref() != Some(song_id)
            || !matches!(task.phase, CloudPhase::Published | CloudPhase::Complete)
        {
            return Err(Error::State("cloud source association changed"));
        }
        task.source.open()?;
        self.guard()?;
        Ok(task.source.path)
    }
    pub(crate) fn removal_source(
        &self,
        id: &str,
        song_id: &str,
    ) -> Result<(
        std::path::PathBuf,
        crate::media::integrity::FileIntegrity,
        bool,
    )> {
        let task = self.read(id)?;
        if task.song_id.as_deref() != Some(song_id)
            || !matches!(task.phase, CloudPhase::Published | CloudPhase::Complete)
        {
            return Err(Error::State("cloud source association changed"));
        }
        let cue = task
            .attachments
            .iter()
            .any(|a| a.kind == AttachmentKind::Cue);
        let integrity = crate::media::integrity::FileIntegrity {
            bytes: task.source.bytes,
            sha256: task.source.sha256,
        };
        self.guard()?;
        Ok((task.source.path, integrity, cue))
    }
    pub fn list(&self) -> Result<Vec<CloudView>> {
        let keys = self.session.with_token(&self.token, |_| {
            Ok(self
                .store
                .values(&self.owner, JOBS)?
                .into_keys()
                .collect::<Vec<_>>())
        })?;
        keys.iter().map(|id| self.get(id)).collect()
    }
    pub async fn create(&self, source: &Path, attachments: AttachmentInputs) -> Result<CloudView> {
        let task = self
            .prepare_task(
                source,
                attachments,
                uuid::Uuid::new_v4().simple().to_string(),
            )
            .await?;
        self.save(&task)?;
        self.view(&task)
    }
    pub(super) async fn prepare_task(
        &self,
        source: &Path,
        attachments: AttachmentInputs,
        id: String,
    ) -> Result<Task> {
        self.guard()?;
        let path = source.to_owned();
        let uploads = self.clone();
        tokio::task::spawn_blocking(move || {
            let path = path.canonicalize()?;
            if path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("ncm"))
            {
                return Err(Error::InvalidInput(
                    "official cloud workflow excludes NCM uploads",
                ));
            }
            let mut file = crate::media::open_regular(&path)?;
            let bytes = file.metadata()?.len();
            if bytes > NEW_AUDIO_LIMIT {
                // Check only preexisting oversized tasks here. An exact durable
                // creation receipt already won before this worker opened a file.
                if bytes <= super::super::source::MAX_RECORDED_BYTES
                    && let Some(name) = path.to_str()
                    && uploads.queued_source(name, bytes, None)?
                {
                    let actual = Source::inspect_open(path.clone(), &mut file, None)?;
                    if actual.bytes == bytes
                        && uploads.queued_source(name, bytes, Some(&actual.sha256))?
                    {
                        return Err(Error::CloudUploadAlreadyQueued);
                    }
                }
                return Err(Error::CloudUploadTooLarge);
            }
            let source = Source::inspect_open(path, &mut file, Some(NEW_AUDIO_LIMIT))?;
            types::file_name(&source)?;
            let info = uploads.probe.inspect(&source.path)?;
            if info.container.eq_ignore_ascii_case("ncm") {
                return Err(Error::Media("encrypted cloud input"));
            }
            let canonical = source.path.canonicalize()?;
            let mut after = crate::media::open_regular(&canonical)?;
            if Source::inspect_open(canonical, &mut after, Some(NEW_AUDIO_LIMIT))? != source {
                return Err(Error::Media("upload input changed during probe"));
            }
            let attachments = attachments::inspect(attachments)?;
            let task = Task {
                id,
                source,
                info,
                phase: CloudPhase::Queued,
                decision: None,
                allocation: None,
                song_id: None,
                attachments,
                next_check_ms: 0,
                error: None,
                retry_phase: None,
                metadata: None,
                publication: None,
                account_item: None,
            };
            task.validate()?;
            Ok(task)
        })
        .await
        .map_err(|_| Error::State("cloud input worker stopped"))?
    }
    fn queued_source(&self, path: &str, bytes: u64, sha256: Option<&str>) -> Result<bool> {
        self.session.with_token(&self.token, |_| {
            self.store
                .visible_cloud_upload_source(&self.owner, path, bytes, sha256)
        })
    }
    /// Atomically order a remote write intent after any durable user control.
    pub(super) fn begin(&self, task: &mut Task, phase: CloudPhase) -> Result<()> {
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, CONTROL, &task.id)?.is_some() {
                    return Err(Error::Cancelled);
                }
                task.phase = phase;
                task.validate()?;
                tx.put(&self.owner, JOBS, &task.id, &serde_json::to_value(&*task)?)
            })
        })
    }
    pub(super) fn rejected<T>(
        &self,
        task: &mut Task,
        previous: CloudPhase,
        result: Result<T>,
    ) -> Result<T> {
        self.guard()?;
        if let Err(Error::Service { code, .. }) = &result {
            task.phase = CloudPhase::Failed;
            task.retry_phase = Some(previous);
            task.error = Some(UploadIssue {
                kind: "service_rejected".into(),
                code: code.as_i64(),
            });
            self.save(task)?;
        }
        result
    }
    pub(super) async fn verify_inputs(&self, task: &Task) -> Result<()> {
        task.source.open_async().await?;
        for a in &task.attachments {
            if !matches!(
                a.phase,
                AttachmentPhase::Uploaded | AttachmentPhase::Omitted
            ) {
                a.source.open_async().await?;
            }
        }
        self.guard()
    }
}
