use super::{types::Task, *};
use crate::{
    account::{AccountSession, SessionToken},
    cloud::nos::{NosBackend, NosUploads, ReqwestNosBackend},
    playlist::PlaylistService,
    storage::{OperationLock, StateStore},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct PlaylistCovers {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) owner: String,
    pub(super) store: Arc<StateStore>,
    pub(super) nos: NosUploads,
    pub(super) playlists: PlaylistService,
}
impl PlaylistCovers {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_backend(session, Arc::new(ReqwestNosBackend::new()?), 256 * 1024)
    }
    pub fn with_backend(
        session: Arc<AccountSession>,
        backend: Arc<dyn NosBackend>,
        chunk_size: usize,
    ) -> Result<Self> {
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let store = session.persistent_store()?;
        let nos = NosUploads::with_backend(session.clone(), backend, chunk_size)?;
        let playlists = PlaylistService::new(session.clone())?;
        session.with_token(&token, |_| Ok(()))?;
        Ok(Self {
            session,
            token,
            owner,
            store,
            nos,
            playlists,
        })
    }
    pub(super) fn guard(&self, token: &SessionToken) -> Result<()> {
        if token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(token, |_| Ok(()))
    }
    pub(super) fn lock(&self, token: &SessionToken, id: &str) -> Result<OperationLock> {
        self.guard(token)?;
        types::valid_id(id)?;
        OperationLock::acquire(
            self.store.directory(),
            "playlist-cover",
            &format!("{}:{id}", self.owner),
        )
    }
    pub(super) fn read(&self, token: &SessionToken, id: &str) -> Result<Task> {
        self.guard(token)?;
        types::valid_id(id)?;
        let task: Task = self.session.with_token(token, |_| {
            Ok(serde_json::from_value(
                self.store
                    .get(&self.owner, TASKS, id)?
                    .ok_or(Error::State("playlist cover task not found"))?,
            )?)
        })?;
        task.validate()?;
        if task.id != id {
            return Err(Error::State("cover task identity mismatch"));
        }
        Ok(task)
    }
    pub(super) fn save(&self, token: &SessionToken, task: &Task) -> Result<()> {
        self.guard(token)?;
        task.validate()?;
        self.session
            .save_result(token, TASKS, &task.id, &serde_json::to_value(task)?)
    }
    pub(super) fn view(&self, task: &Task) -> Result<CoverTaskView> {
        let transfer = self.nos.find(&task.id)?;
        Ok(CoverTaskView {
            id: task.id.clone(),
            playlist_id: task.playlist_id.clone(),
            phase: task.phase,
            crop: task.crop,
            width: task.width,
            height: task.height,
            bytes: transfer.as_ref().map_or(0, |v| v.bytes),
            total: task.total,
            transfer: transfer.as_ref().map(|v| v.status),
            binding: task.binding,
            error: task
                .error
                .clone()
                .or_else(|| transfer.and_then(|v| v.error)),
            may_leave_object: task.may_leave_object,
            cleanup_pending: task.phase.terminal()
                && (!task.source.is_empty() || !task.encoded.is_empty()),
        })
    }
    pub fn get(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        self.view(&self.read(token, id)?)
    }
    pub fn list(&self, token: &SessionToken) -> Result<Vec<CoverTaskView>> {
        self.guard(token)?;
        let keys = self.store.values(&self.owner, TASKS)?.into_keys();
        keys.map(|id| self.get(token, &id)).collect()
    }
    pub(super) async fn check_target(
        &self,
        token: &SessionToken,
        playlist_id: &str,
        revision: &str,
    ) -> Result<()> {
        self.guard(token)?;
        let detail = self.playlists.detail(token, playlist_id).await?;
        detail.writable(&self.owner)?;
        if detail.revision != revision {
            return Err(Error::QueueChanged);
        }
        if self
            .playlists
            .operations(token)?
            .iter()
            .any(|r| r.phase.pending() && r.playlist_id.as_deref() == Some(playlist_id))
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
    /// The host supplies an explicitly selected local image. Persist immutable
    /// bytes before presenting the cropper; later edits to that file cannot
    /// alter an upload the user has already reviewed.
    pub async fn select(
        &self,
        token: &SessionToken,
        playlist_id: &str,
        revision: &str,
        bytes: Vec<u8>,
    ) -> Result<CoverTaskView> {
        self.guard(token)?;
        let (image, bytes) =
            tokio::task::spawn_blocking(move || inspect_image(&bytes).map(|image| (image, bytes)))
                .await
                .map_err(|_| Error::State("cover image worker stopped"))??;
        self.check_target(token, playlist_id, revision).await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let task = Task {
            id: id.clone(),
            playlist_id: playlist_id.into(),
            revision: revision.into(),
            width: image.width,
            height: image.height,
            source_sha256: hex::encode(Sha256::digest(&bytes)),
            source: STANDARD.encode(bytes),
            crop: None,
            encoded: String::new(),
            md5: String::new(),
            sha256: String::new(),
            total: 0,
            phase: CoverPhase::Draft,
            allocation: None,
            binding_id: uuid::Uuid::new_v4().simple().to_string(),
            binding: None,
            error: None,
            may_leave_object: false,
        };
        task.validate()?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let mut active = 0;
                for value in tx.values(&self.owner, TASKS)?.into_values() {
                    let old: Task = serde_json::from_value(value)?;
                    if !old.phase.terminal() {
                        active += 1;
                        if old.playlist_id == playlist_id {
                            return Err(Error::MutationBusy);
                        }
                    }
                }
                if active >= 16 {
                    return Err(Error::State("too many pending cover edits"));
                }
                tx.put(&self.owner, TASKS, &id, &serde_json::to_value(&task)?)
            })
        })?;
        self.view(&task)
    }
    pub async fn select_file(
        &self,
        token: &SessionToken,
        playlist_id: &str,
        revision: &str,
        path: &Path,
    ) -> Result<CoverTaskView> {
        self.guard(token)?;
        let bytes = Self::read_file(path).await?;
        self.select(token, playlist_id, revision, bytes).await
    }
    async fn read_file(path: &Path) -> Result<Vec<u8>> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = crate::media::open_regular(&path)?;
            if file.metadata()?.len() > image::INPUT_LIMIT as u64 {
                return Err(Error::InvalidInput("cover exceeds 5MiB"));
            }
            let mut bytes = Vec::new();
            file.take(image::INPUT_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::State("cover file worker stopped"))?
    }
    pub async fn replace_file(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        path: &Path,
    ) -> Result<CoverTaskView> {
        self.guard(token)?;
        let bytes = Self::read_file(path).await?;
        let (image, bytes) =
            tokio::task::spawn_blocking(move || inspect_image(&bytes).map(|image| (image, bytes)))
                .await
                .map_err(|_| Error::State("cover image worker stopped"))??;
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if !matches!(task.phase, CoverPhase::Draft | CoverPhase::Prepared) {
            return Err(Error::State("cover image upload has already started"));
        }
        if task.revision != revision {
            return Err(Error::QueueChanged);
        }
        self.check_target(token, &task.playlist_id, revision)
            .await?;
        task.width = image.width;
        task.height = image.height;
        task.source_sha256 = hex::encode(Sha256::digest(&bytes));
        task.source = STANDARD.encode(bytes);
        task.crop = None;
        task.encoded.clear();
        task.md5.clear();
        task.sha256.clear();
        task.total = 0;
        task.phase = CoverPhase::Draft;
        task.error = None;
        self.save(token, &task)?;
        self.view(&task)
    }
    pub fn image(&self, token: &SessionToken, id: &str) -> Result<CoverImage> {
        let task = self.read(token, id)?;
        if task.phase.terminal() {
            return Err(Error::State("cover edit already finished"));
        }
        inspect_image(&Self::source(&task)?)
    }
    fn source(task: &Task) -> Result<Vec<u8>> {
        let bytes = STANDARD
            .decode(&task.source)
            .map_err(|_| Error::State("invalid saved cover image"))?;
        if hex::encode(Sha256::digest(&bytes)) != task.source_sha256 {
            return Err(Error::Media("saved cover source changed"));
        }
        Ok(bytes)
    }
    pub fn prepare(
        &self,
        token: &SessionToken,
        id: &str,
        crop: CoverCrop,
    ) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if !matches!(task.phase, CoverPhase::Draft | CoverPhase::Prepared) {
            return Err(Error::State("cover image upload has already started"));
        }
        let bytes = image::crop(&Self::source(&task)?, crop)?;
        task.crop = Some(crop);
        task.total = bytes.len() as u64;
        task.md5 = hex::encode(md5::Md5::digest(&bytes));
        task.sha256 = hex::encode(Sha256::digest(&bytes));
        task.encoded = STANDARD.encode(bytes);
        task.phase = CoverPhase::Prepared;
        self.save(token, &task)?;
        self.view(&task)
    }
    fn file_path(&self, task: &Task) -> PathBuf {
        self.store
            .directory()
            .join(format!("playlist-cover-{}.jpg", task.id))
    }
    pub(super) fn materialize(&self, task: &Task) -> Result<PathBuf> {
        let path = self.file_path(task);
        let bytes = STANDARD
            .decode(&task.encoded)
            .map_err(|_| Error::State("invalid cropped cover"))?;
        if bytes.len() as u64 != task.total
            || hex::encode(Sha256::digest(&bytes)) != task.sha256
            || hex::encode(md5::Md5::digest(&bytes)) != task.md5
        {
            return Err(Error::Media("saved cropped cover changed"));
        }
        if !path.try_exists()? {
            let mut file = tempfile::NamedTempFile::new_in(self.store.directory())?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&path)
                .map_err(|e| Error::Io(e.error))?;
        }
        self.verify_file(task)?;
        Ok(path)
    }
    fn verify_file(&self, task: &Task) -> Result<()> {
        let path = self.file_path(task);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::Media("cover upload file was replaced"));
        }
        let actual = crate::media::integrity::inspect_file(&path)?;
        if actual.sha256 != task.sha256 || actual.bytes != task.total {
            return Err(Error::Media("cover upload file changed"));
        }
        Ok(())
    }
    /// Local cleanup is repeatable after a crash, and refuses foreign content.
    /// The journal, hashes and remote responsibility remain after byte removal.
    pub fn cleanup(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if !task.phase.terminal() {
            return Err(Error::State("cover edit is not terminal"));
        }
        let path = self.file_path(&task);
        if path.try_exists()? {
            self.verify_file(&task)?;
            std::fs::remove_file(path)?;
        }
        task.source.clear();
        task.encoded.clear();
        self.save(token, &task)?;
        self.view(&task)
    }
}
