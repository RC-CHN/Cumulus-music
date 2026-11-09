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
pub(super) struct TaskDestination {
    pub(super) destination: DownloadDestination,
    pub(super) root: std::path::PathBuf,
    pub(super) identity: files::Identity,
}

/// Owns a newly created, still private work directory until its task is durable.
pub(super) struct StagedTask {
    task: Option<DownloadTask>,
    _lock: OperationLock,
}

impl StagedTask {
    pub(super) fn task(&self) -> &DownloadTask {
        self.task.as_ref().expect("staged download")
    }

    pub(super) fn commit(mut self) -> DownloadTask {
        self.task.take().expect("staged download")
    }
}

impl Drop for StagedTask {
    fn drop(&mut self) {
        if let Some(task) = &self.task
            && files::work(task).is_ok()
        {
            let _ = fs::remove_dir(&task.work);
        }
    }
}

pub(super) fn creation_clock() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("invalid creation clock"))?
            .as_millis(),
    )
    .map_err(|_| Error::State("creation clock overflow"))
}

impl DownloadService {
    pub(crate) fn has_transfer_control(&self, id: &str) -> Result<bool> {
        Ok(self.store.get(&self.owner, CONTROL, id)?.is_some())
    }

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
        self.get_for(&self.session.token()?, id)
    }
    pub(crate) fn get_for(&self, token: &SessionToken, id: &str) -> Result<DownloadTask> {
        self.guard(token)?;
        self.read(id)
    }
    pub(crate) fn get_ordered_for(
        &self,
        token: &SessionToken,
        ids: &[String],
    ) -> Result<Vec<DownloadTask>> {
        self.guard(token)?;
        for id in ids {
            valid_id(id)?;
        }
        let records = self.store.get_ordered(&self.owner, KIND, ids)?;
        if records.len() != ids.len() {
            return Err(Error::State("download task count mismatch"));
        }
        let result = ids
            .iter()
            .zip(records)
            .map(|(id, value)| {
                let task: DownloadTask = serde_json::from_value(
                    value.ok_or(Error::State("download does not belong to this account"))?,
                )?;
                if &task.id != id {
                    return Err(Error::State("download task ID mismatch"));
                }
                Ok(task)
            })
            .collect::<Result<_>>()?;
        self.guard(token)?;
        Ok(result)
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
    /// A direct catalog row has no download task ID. Only an unambiguous
    /// completed task belonging to this exact account and song may supply it.
    pub(crate) fn published_catalog_song(
        session: &AccountSession,
        token: &SessionToken,
        song: &str,
    ) -> Result<Option<DownloadTask>> {
        let store = session.persistent_store()?;
        let owner = session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let value = store.completed_song_publication(&owner, song)?;
        session.check_generation(token)?;
        value
            .map(|(id, value)| {
                let task: DownloadTask = serde_json::from_value(value)?;
                if task.id != id
                    || task.song_id != song
                    || task.origin != DownloadOrigin::Song
                    || task.status != DownloadStatus::Complete
                {
                    return Err(Error::State("download publication identity mismatch"));
                }
                Ok(task)
            })
            .transpose()
    }

    /// A downloaded queue row already carries a task key. Check that key in
    /// the current account and bind it to its published catalogue song.
    /// Collection uses song identity only, so never reopen the local file
    /// or acquire a playback/download grant here.
    pub(crate) fn queue_collection_tasks_match(
        session: &AccountSession,
        token: &SessionToken,
        wanted: &[(&str, &str)],
    ) -> Result<bool> {
        #[derive(serde::Deserialize)]
        struct Publication {
            id: String,
            song_id: String,
            #[serde(default)]
            origin: DownloadOrigin,
            status: DownloadStatus,
        }
        if wanted.is_empty() {
            return Ok(true);
        }
        let store = session.persistent_store()?;
        let owner = session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let matches = store.read_snapshot(|tx| {
            for (id, song) in wanted {
                if valid_id(id).is_err() {
                    return Ok(false);
                }
                let Some(value) = tx.get(&owner, KIND, id)? else {
                    return Ok(false);
                };
                let record: Publication = serde_json::from_value(value)?;
                if record.id != *id
                    || record.song_id != *song
                    || record.origin != DownloadOrigin::Song
                    || record.status != DownloadStatus::Complete
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
        session.check_generation(token)?;
        Ok(matches)
    }

    /// Read only the catalog-song publications relevant to a native playback
    /// preflight; this does not construct a transfer backend or renew a grant.
    pub(crate) fn queue_publications(
        session: &AccountSession,
        token: &SessionToken,
        wanted: &std::collections::HashSet<&str>,
    ) -> Result<Vec<DownloadTask>> {
        let store = session.persistent_store()?;
        let owner = session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let mut out = Vec::new();
        for (id, value) in store.values(&owner, KIND)? {
            if !value["song_id"]
                .as_str()
                .is_some_and(|id| wanted.contains(id))
            {
                continue;
            }
            let task: DownloadTask = serde_json::from_value(value)?;
            if task.id != id {
                return Err(Error::State("download task ID mismatch"));
            }
            if task.origin == DownloadOrigin::Song && task.status == DownloadStatus::Complete {
                out.push(task);
            }
        }
        session.check_generation(token)?;
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
        self.create_default_from_with(song_id, options, origin, song, None, || {})
    }

    pub(crate) fn create_guarded_song(
        &self,
        song_id: &Value,
        options: DownloadOptions,
        permission: &dyn Fn() -> Result<()>,
        committed: impl FnOnce(),
    ) -> Result<DownloadTask> {
        self.create_default_from_with(
            song_id,
            options,
            DownloadOrigin::Song,
            None,
            Some(permission),
            committed,
        )
    }

    fn create_default_from_with(
        &self,
        song_id: &Value,
        options: DownloadOptions,
        origin: DownloadOrigin,
        song: Option<Value>,
        permission: Option<&dyn Fn() -> Result<()>>,
        committed: impl FnOnce(),
    ) -> Result<DownloadTask> {
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        self.guard(&self.session.token()?)?;
        if let Some(permission) = permission {
            permission()?;
        }
        directory_gate::require(&self.store, &self.owner, None)?;
        let settings: crate::download::directory::DirectorySettings = serde_json::from_value(
            self.store
                .get(&self.owner, directory_gate::SETTINGS, "settings")?
                .ok_or(Error::State("download directory is not configured"))?,
        )?;
        let task = self.create_inner_with(
            song_id,
            DownloadDestination::Named {
                root: settings.root,
                path_type: settings.path_type,
                file_type: settings.file_type,
            },
            options,
            origin,
            song,
            permission,
        )?;
        committed();
        Ok(task)
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
        destination: DownloadDestination,
        options: DownloadOptions,
        origin: DownloadOrigin,
        song: Option<Value>,
    ) -> Result<DownloadTask> {
        self.create_inner_with(song_id, destination, options, origin, song, None)
    }

    fn create_inner_with(
        &self,
        song_id: &Value,
        mut destination: DownloadDestination,
        options: DownloadOptions,
        origin: DownloadOrigin,
        song: Option<Value>,
        permission: Option<&dyn Fn() -> Result<()>>,
    ) -> Result<DownloadTask> {
        let token = self.session.token()?;
        self.guard(&token)?;
        directory_gate::require(&self.store, &self.owner, None)?;
        let song_id = identifier(song_id)?;
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
        let destination = TaskDestination {
            identity: files::identity(&root)?,
            destination,
            root,
        };
        let staged = self.stage_task(
            uuid::Uuid::new_v4().simple().to_string(),
            creation_clock()?,
            song_id,
            &destination,
            options,
            (origin, song),
        )?;
        files::sync(&destination.root)?;
        if let Some(permission) = permission {
            permission()?;
        }
        self.save_current(&token, staged.task())?;
        Ok(staged.commit())
    }

    pub(super) fn stage_task(
        &self,
        id: String,
        created_ms: u64,
        song_id: String,
        location: &TaskDestination,
        mut options: DownloadOptions,
        (origin, song): (DownloadOrigin, Option<Value>),
    ) -> Result<StagedTask> {
        options.cover &= options.tags || options.container != OutputContainer::Audio;
        files::directory(&location.root, &location.identity)?;
        let lock = self.lock(&id)?;
        let work = location.root.join(format!(".cumulus-{id}"));
        crate::storage::private_directory_builder().create(&work)?;
        let task = DownloadTask {
            id,
            song_id,
            origin,
            options,
            destination: location.destination.clone(),
            target: None,
            work: work.clone(),
            status: DownloadStatus::Queued,
            phase: DownloadPhase::Authorize,
            created_ms,
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
            root: location.root.clone(),
            root_identity: location.identity.clone(),
            work_identity: files::identity(&work)?,
            target_parent: None,
            prepared: None,
            cover: None,
            lyric: None,
        };
        Ok(StagedTask {
            task: Some(task),
            _lock: lock,
        })
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
        if task.needs_transfer_restart() {
            return Err(Error::State(
                "download integrity failure requires an explicit restart",
            ));
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
                if action == DownloadAction::Pause && task.needs_transfer_restart() {
                    return Err(Error::State("corrupt download is already stopped"));
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
                } else if !task.needs_transfer_restart() {
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

#[cfg(test)]
mod admission_tests {
    use super::*;
    use crate::{
        download::directory::DownloadDirectory,
        protocol::api::{Api, ApiFuture},
    };
    use serde_json::json;
    use std::cell::Cell;

    struct Closed;
    impl Api for Closed {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("admission must not authorize a download") })
        }
    }

    #[test]
    fn stale_guard_after_work_directory_preparation_leaves_no_task_or_staged_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path().join("state")).unwrap());
        let session = Arc::new(AccountSession::new(Arc::new(Closed), Some(store.clone())));
        session
            .adopt(
                Arc::new(Closed),
                json!({"userId":25}),
                &session.token().unwrap(),
            )
            .unwrap();
        let out = temp.path().join("out");
        DownloadDirectory::new(session.clone())
            .unwrap()
            .configure(&out, 0, 0)
            .unwrap();
        let service = DownloadService::new(session).unwrap();
        let seen = Cell::new(0);
        let result = service.create_guarded_song(
            &json!(301),
            DownloadOptions::default(),
            &|| {
                let step = seen.get() + 1;
                seen.set(step);
                if step == 1 {
                    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
                    Ok(())
                } else {
                    assert_eq!(step, 2);
                    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
                    assert!(store.values("25", KIND).unwrap().is_empty());
                    Err(Error::StaleResponse)
                }
            },
            || panic!("stale intent was reported as committed"),
        );
        assert!(matches!(result, Err(Error::StaleResponse)));
        assert_eq!(seen.get(), 2);
        assert!(store.values("25", KIND).unwrap().is_empty());
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }
}
