use super::{control::Control, files, *};
use crate::download::directory::gate as directory_gate;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{
        offline::OfflineLibrary,
        transfer::{MediaBackend, ReqwestMediaBackend, TransferManager},
    },
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde_json::Value;
use std::{
    fs,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub(in crate::download) const KIND: &str = "download_tasks_v1";
pub(in crate::download) const CONTROL: &str = "download_controls_v1";

#[derive(Clone)]
pub struct DownloadService {
    pub(super) session: Arc<AccountSession>,
    bound_token: SessionToken,
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
    pub(super) backend: Arc<dyn MediaBackend>,
    pub(super) transfer: Arc<TransferManager>,
    pub(super) offline: Arc<OfflineLibrary>,
}
impl DownloadService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_backend(
            session,
            Arc::new(ReqwestMediaBackend::new(Duration::from_secs(30))?),
            512 * 1024 * 1024,
        )
    }
    pub fn with_backend(
        session: Arc<AccountSession>,
        backend: Arc<dyn MediaBackend>,
        max_bytes: u64,
    ) -> Result<Self> {
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let store = session.persistent_store()?;
        Ok(Self {
            transfer: Arc::new(TransferManager::with_backend(
                store.clone(),
                owner.clone(),
                backend.clone(),
                max_bytes,
            )?),
            offline: Arc::new(OfflineLibrary::new(session.clone())?),
            session,
            bound_token: token,
            store,
            owner,
            backend,
        })
    }
    pub(super) fn guard(&self, token: &SessionToken) -> Result<()> {
        if token != &self.bound_token {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(token, |uid| {
            if uid == self.owner {
                Ok(())
            } else {
                Err(Error::StaleResponse)
            }
        })
    }
    pub(super) fn lock(&self, id: &str) -> Result<OperationLock> {
        valid_id(id)?;
        OperationLock::acquire(
            self.store.directory(),
            "download-task",
            &serde_json::to_string(&(&self.owner, id))?,
        )
    }
    pub(super) fn read(&self, id: &str) -> Result<DownloadTask> {
        valid_id(id)?;
        let task: DownloadTask = serde_json::from_value(
            self.store
                .get(&self.owner, KIND, id)?
                .ok_or(Error::State("download does not belong to this account"))?,
        )?;
        if task.id != id {
            return Err(Error::State("download task ID mismatch"));
        }
        Ok(task)
    }
    pub fn get(&self, id: &str) -> Result<DownloadTask> {
        self.guard(&self.session.token()?)?;
        self.read(id)
    }
    pub fn list(&self) -> Result<Vec<DownloadTask>> {
        let token = self.session.token()?;
        self.guard(&token)?;
        let mut out: Vec<DownloadTask> = self
            .store
            .values(&self.owner, KIND)?
            .into_values()
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .collect::<Result<_>>()?;
        out.sort_by(|a, b| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
        self.guard(&token)?;
        Ok(out)
    }
    pub(super) fn save(&self, task: &DownloadTask) -> Result<()> {
        self.store
            .put(&self.owner, KIND, &task.id, &serde_json::to_value(task)?)
    }
    pub(super) fn save_current(&self, token: &SessionToken, task: &DownloadTask) -> Result<()> {
        self.session.with_token(token, |uid| {
            if uid != self.owner {
                return Err(Error::StaleResponse);
            }
            self.save(task)
        })
    }
    pub fn create_default(
        &self,
        song_id: &Value,
        options: DownloadOptions,
    ) -> Result<DownloadTask> {
        self.create_default_from(song_id, options, DownloadOrigin::Song, None)
    }
    pub(super) fn create_default_from(
        &self,
        song_id: &Value,
        options: DownloadOptions,
        origin: DownloadOrigin,
        song: Option<Value>,
    ) -> Result<DownloadTask> {
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        self.guard(&self.session.token()?)?;
        directory_gate::require(&self.store, &self.owner, None)?;
        let settings: crate::download::directory::DirectorySettings = serde_json::from_value(
            self.store
                .get(&self.owner, directory_gate::SETTINGS, "settings")?
                .ok_or(Error::State("download directory is not configured"))?,
        )?;
        self.create_inner(
            song_id,
            DownloadDestination::Named {
                root: settings.root,
                path_type: settings.path_type,
                file_type: settings.file_type,
            },
            options,
            origin,
            song,
        )
    }
    pub fn create(
        &self,
        song_id: &Value,
        destination: DownloadDestination,
        options: DownloadOptions,
    ) -> Result<DownloadTask> {
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        self.create_inner(song_id, destination, options, DownloadOrigin::Song, None)
    }
    pub(super) fn create_inner(
        &self,
        song_id: &Value,
        mut destination: DownloadDestination,
        mut options: DownloadOptions,
        origin: DownloadOrigin,
        song: Option<Value>,
    ) -> Result<DownloadTask> {
        let token = self.session.token()?;
        self.guard(&token)?;
        directory_gate::require(&self.store, &self.owner, None)?;
        let song_id = identifier(song_id)?;
        options.cover &= options.tags || options.container != OutputContainer::Audio;
        let root = match &mut destination {
            DownloadDestination::Path { path } => {
                let full = std::path::absolute(&*path)?;
                let parent = full
                    .parent()
                    .ok_or(Error::InvalidInput("download requires a parent directory"))?;
                let name = full
                    .file_name()
                    .ok_or(Error::InvalidInput("download requires a filename"))?;
                fs::create_dir_all(parent)?;
                let root = parent.canonicalize()?;
                *path = root.join(name);
                files::absent(path)?;
                root
            }
            DownloadDestination::Named {
                root,
                path_type,
                file_type,
            } => {
                if *path_type > 2 || *file_type > 2 {
                    return Err(Error::InvalidInput("download layout must be 0..2"));
                }
                fs::create_dir_all(&*root)?;
                *root = root.canonicalize()?;
                root.clone()
            }
        };
        if root.to_str().is_none() {
            return Err(Error::InvalidInput("download directory must be UTF-8"));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let _lock = self.lock(&id)?;
        let work = root.join(format!(".cumulus-{id}"));
        let root_identity = files::identity(&root)?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&work)?;
        let task = DownloadTask {
            id,
            song_id,
            origin,
            options,
            destination,
            target: None,
            work: work.clone(),
            status: DownloadStatus::Queued,
            phase: DownloadPhase::Authorize,
            created_ms: u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::State("invalid creation clock"))?
                    .as_millis(),
            )
            .map_err(|_| Error::State("creation clock overflow"))?,
            completed_ms: None,
            bytes: 0,
            authorization_count: 0,
            resource: None,
            song,
            encrypted: false,
            official_encryption: false,
            raw_integrity: None,
            output_integrity: None,
            cleanup_pending: false,
            last_error: None,
            service_code: None,
            lyrics_status: LyricsStatus::Pending,
            lyrics_error: None,
            file_operation: None,
            root,
            root_identity,
            work_identity: files::identity(&work)?,
            target_parent: None,
            prepared: None,
            cover: None,
            lyric: None,
        };
        files::sync(&task.root)?;
        if let Err(e) = self.save_current(&token, &task) {
            if files::work(&task).is_ok() {
                let _ = fs::remove_dir(&work);
            }
            return Err(e);
        }
        Ok(task)
    }
    pub fn enqueue(&self, id: &str) -> Result<DownloadTask> {
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        directory_gate::require(&self.store, &self.owner, None)?;
        let token = self.session.token()?;
        self.guard(&token)?;
        let _lock = self.lock(id)?;
        let mut task = self.read(id)?;
        if matches!(
            task.status,
            DownloadStatus::Complete | DownloadStatus::Cancelled | DownloadStatus::Cancelling
        ) {
            return Err(Error::State("terminal task cannot be queued"));
        }
        task.status = DownloadStatus::Queued;
        task.last_error = None;
        task.service_code = None;
        self.session.with_token(&token, |_| {
            self.store.transaction(|tx| {
                tx.delete(&self.owner, CONTROL, id)?;
                tx.put(&self.owner, KIND, id, &serde_json::to_value(&task)?)?;
                Ok(())
            })
        })?;
        Ok(task)
    }
    pub fn request(&self, id: &str, action: DownloadAction) -> Result<DownloadTask> {
        let token = self.session.token()?;
        self.guard(&token)?;
        self.session.with_token(&token, |_| {
            self.store.transaction(|tx| {
                let task: DownloadTask = serde_json::from_value(
                    tx.get(&self.owner, KIND, id)?
                        .ok_or(Error::State("unknown download task"))?,
                )?;
                if matches!(
                    task.status,
                    DownloadStatus::Complete | DownloadStatus::Cancelled
                ) || files::published(&task)?
                {
                    return Err(Error::State(
                        "published or terminal download cannot be paused/cancelled",
                    ));
                }
                let old = Control::read_tx(tx, &self.owner, id)?;
                if old != Some(DownloadAction::Cancel) {
                    tx.put(&self.owner, CONTROL, id, &serde_json::to_value(action)?)?;
                }
                Ok(())
            })
        })?;
        match self.lock(id) {
            Ok(_lock) => {
                if directory_gate::active(&self.store, &self.owner)?.is_some() {
                    return self.get(id);
                }
                let mut task = self.read(id)?;
                if matches!(
                    task.status,
                    DownloadStatus::Complete | DownloadStatus::Cancelled
                ) || files::published(&task)?
                {
                    return Ok(task);
                }
                let pending: Option<DownloadAction> = self
                    .store
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if pending == Some(DownloadAction::Cancel) {
                    self.cancel_task(&token, &mut task)?;
                } else {
                    task.status = DownloadStatus::Paused;
                    self.save_current(&token, &task)?;
                }
            }
            Err(Error::MutationBusy) => (),
            Err(e) => return Err(e),
        }
        self.get(id)
    }
}
fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid download task ID"));
    }
    Ok(())
}
