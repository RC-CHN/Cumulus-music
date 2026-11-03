//! Avatar cropping, scoped object transfer, and one durable account binding.
mod model;
mod transfer;

pub use crate::playlist::cover::{CoverCrop as AvatarCrop, CoverImage as AvatarImage};
pub use model::{AvatarIssue, AvatarPhase, AvatarTaskView};

use super::{AccountProfile, SelfProfileView, avatar as safe_avatar};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    cloud::nos::{NosBackend, NosControl, NosStatus, NosUploads, ReqwestNosBackend},
    playlist::cover::{INPUT_LIMIT, crop_square_image, inspect_image},
    storage::{OperationLock, StateStore},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use model::{Task, valid_id};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

const TASKS: &str = "account-avatar-v1";

pub struct AvatarEditor {
    session: Arc<AccountSession>,
    token: SessionToken,
    owner: String,
    store: Arc<StateStore>,
    nos: NosUploads,
}

impl AvatarEditor {
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
        session.with_token(&token, |_| Ok(()))?;
        Ok(Self {
            session,
            token,
            owner,
            store,
            nos,
        })
    }

    fn guard(&self, token: &SessionToken) -> Result<()> {
        if token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(token, |_| Ok(()))
    }

    fn lock(&self, token: &SessionToken) -> Result<OperationLock> {
        self.guard(token)?;
        OperationLock::acquire(self.store.directory(), TASKS, &self.owner)
    }

    fn read(&self, token: &SessionToken, id: &str) -> Result<Task> {
        self.guard(token)?;
        valid_id(id)?;
        self.session.with_token(token, |uid| {
            let task: Task = serde_json::from_value(
                self.store
                    .get(uid, TASKS, id)?
                    .ok_or(Error::State("avatar task not found"))?,
            )?;
            task.validate()?;
            if task.id != id {
                return Err(Error::State("avatar task identity mismatch"));
            }
            Ok(task)
        })
    }

    fn save(&self, token: &SessionToken, task: &Task) -> Result<()> {
        task.validate()?;
        self.session
            .save_result(token, TASKS, &task.id, &serde_json::to_value(task)?)
    }

    fn view(&self, token: &SessionToken, task: &Task) -> Result<AvatarTaskView> {
        let transfer = self.nos.find(&task.id)?;
        let view = AvatarTaskView {
            id: task.id.clone(),
            phase: task.phase,
            crop: task.crop,
            width: task.width,
            height: task.height,
            bytes: transfer.as_ref().map_or(0, |child| child.bytes),
            total: task.total,
            transfer: transfer.as_ref().map(|child| child.status),
            error: task.error.or_else(|| match task.phase {
                AvatarPhase::Allocating => Some(AvatarIssue::AllocationUncertain),
                AvatarPhase::Binding => Some(AvatarIssue::BindUncertain),
                AvatarPhase::Uploading
                    if transfer.is_some_and(|child| {
                        matches!(child.status, NosStatus::Error | NosStatus::Ambiguous)
                    }) =>
                {
                    Some(AvatarIssue::UploadFailed)
                }
                _ => None,
            }),
            may_leave_object: task.may_leave_object,
            cleanup_pending: task.phase.terminal() && task.has_local_bytes(),
            metadata_pending: false,
            observed_revision: task.observed_revision(),
            observed_avatar_url: task.observed.clone(),
        };
        self.guard(token)?;
        Ok(view)
    }

    pub fn get(&self, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
        self.view(token, &self.read(token, id)?)
    }

    pub fn list(&self, token: &SessionToken) -> Result<Vec<AvatarTaskView>> {
        self.guard(token)?;
        let mut tasks = self.session.with_token(token, |uid| {
            self.store
                .values(uid, TASKS)?
                .into_iter()
                .map(|(id, value)| {
                    let task: Task = serde_json::from_value(value)?;
                    task.validate()?;
                    if id != task.id {
                        return Err(Error::State("avatar task identity mismatch"));
                    }
                    Ok(task)
                })
                .collect::<Result<Vec<_>>>()
        })?;
        tasks.sort_by_key(|task| {
            (
                task.phase.terminal() && !task.has_local_bytes(),
                std::cmp::Reverse(task.created_at),
            )
        });
        tasks
            .into_iter()
            .take(20)
            .map(|task| self.view(token, &task))
            .collect()
    }

    pub async fn select_file(&self, token: &SessionToken, path: &Path) -> Result<AvatarTaskView> {
        self.guard(token)?;
        let path = path.to_owned();
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let file = crate::media::open_regular(&path)?;
            if file.metadata()?.len() > INPUT_LIMIT as u64 {
                return Err(Error::InvalidInput("cover exceeds 5MiB"));
            }
            let mut bytes = Vec::new();
            file.take(INPUT_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::State("avatar file worker stopped"))??;
        self.select(token, bytes).await
    }

    pub async fn select(&self, token: &SessionToken, bytes: Vec<u8>) -> Result<AvatarTaskView> {
        self.guard(token)?;
        let (image, bytes) =
            tokio::task::spawn_blocking(move || inspect_image(&bytes).map(|image| (image, bytes)))
                .await
                .map_err(|_| Error::State("avatar image worker stopped"))??;
        let _lock = self.lock(token)?;
        let task = Task {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            phase: AvatarPhase::Draft,
            width: image.width,
            height: image.height,
            source_sha: hex::encode(Sha256::digest(&bytes)),
            source: STANDARD.encode(bytes),
            crop: None,
            encoded: String::new(),
            sha256: String::new(),
            md5: String::new(),
            total: 0,
            baseline: None,
            allocation: None,
            imgid: None,
            acknowledged: None,
            observed: None,
            error: None,
            may_leave_object: false,
        };
        task.validate()?;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                for (id, value) in tx.values(uid, TASKS)? {
                    let mut old: Task = serde_json::from_value(value)?;
                    old.validate()?;
                    if id != old.id {
                        return Err(Error::State("avatar task identity mismatch"));
                    }
                    match old.phase {
                        AvatarPhase::Draft | AvatarPhase::Prepared => {
                            // No materialized object/file exists before allocation.
                            old.phase = AvatarPhase::Abandoned;
                            old.source.clear();
                            old.encoded.clear();
                            tx.put(uid, TASKS, &id, &serde_json::to_value(old)?)?;
                        }
                        AvatarPhase::Binding => return Err(Error::MutationUncertain),
                        phase if !phase.terminal() => return Err(Error::MutationBusy),
                        _ if old.has_local_bytes() => {
                            return Err(Error::State("clean up the previous avatar image first"));
                        }
                        _ => (),
                    }
                }
                tx.put(uid, TASKS, &task.id, &serde_json::to_value(&task)?)
            })
        })?;
        self.view(token, &task)
    }

    fn source(task: &Task) -> Result<Vec<u8>> {
        let bytes = STANDARD
            .decode(&task.source)
            .map_err(|_| Error::Media("invalid saved avatar source"))?;
        if hex::encode(Sha256::digest(&bytes)) != task.source_sha {
            return Err(Error::Media("saved avatar source changed"));
        }
        Ok(bytes)
    }

    pub fn image(&self, token: &SessionToken, id: &str) -> Result<AvatarImage> {
        let task = self.read(token, id)?;
        if !matches!(task.phase, AvatarPhase::Draft | AvatarPhase::Prepared) {
            return Err(Error::State("avatar crop is no longer editable"));
        }
        let image = inspect_image(&Self::source(&task)?)?;
        self.guard(token)?;
        Ok(image)
    }

    pub fn prepare(
        &self,
        token: &SessionToken,
        id: &str,
        crop: AvatarCrop,
    ) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let mut task = self.read(token, id)?;
        if !matches!(task.phase, AvatarPhase::Draft | AvatarPhase::Prepared) {
            return Err(Error::State("avatar crop is no longer editable"));
        }
        let bytes = crop_square_image(&Self::source(&task)?, crop)?;
        task.crop = Some(crop);
        task.total = bytes.len() as u64;
        task.sha256 = hex::encode(Sha256::digest(&bytes));
        task.md5 = hex::encode(md5::Md5::digest(&bytes));
        task.encoded = STANDARD.encode(bytes);
        task.phase = AvatarPhase::Prepared;
        self.save(token, &task)?;
        self.view(token, &task)
    }

    fn file(&self, task: &Task) -> PathBuf {
        self.store
            .directory()
            .join(format!("account-avatar-{}.jpg", task.id))
    }

    fn encoded(task: &Task) -> Result<Vec<u8>> {
        let bytes = STANDARD
            .decode(&task.encoded)
            .map_err(|_| Error::Media("invalid saved avatar crop"))?;
        if bytes.len() as u64 != task.total
            || hex::encode(Sha256::digest(&bytes)) != task.sha256
            || hex::encode(md5::Md5::digest(&bytes)) != task.md5
        {
            return Err(Error::Media("saved avatar crop changed"));
        }
        Ok(bytes)
    }

    fn materialize(&self, task: &Task) -> Result<PathBuf> {
        let bytes = Self::encoded(task)?;
        let path = self.file(task);
        if !path.try_exists()? {
            let mut temporary = tempfile::NamedTempFile::new_in(self.store.directory())?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            temporary
                .persist_noclobber(&path)
                .map_err(|error| Error::Io(error.error))?;
        }
        Self::verify_file(task, &path)?;
        Ok(path)
    }

    fn verify_file(task: &Task, path: &Path) -> Result<()> {
        if !std::fs::symlink_metadata(path)?.is_file() {
            return Err(Error::Media("avatar upload file was replaced"));
        }
        let actual = crate::media::integrity::inspect_file(path)?;
        if actual.bytes != task.total || actual.sha256 != task.sha256 {
            return Err(Error::Media("avatar upload file changed"));
        }
        Ok(())
    }

    pub fn abandon(&self, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let mut task = self.read(token, id)?;
        if task.phase == AvatarPhase::Binding {
            return Err(Error::MutationUncertain);
        }
        if !task.phase.terminal() {
            if let Some(child) = self.nos.find(id)?
                && !matches!(child.status, NosStatus::Uploaded | NosStatus::Cancelled)
            {
                self.nos.control(id, NosControl::Cancel)?;
            }
            task.phase = AvatarPhase::Abandoned;
            self.save(token, &task)?;
        }
        self.view(token, &task)
    }

    pub fn cleanup(&self, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let mut task = self.read(token, id)?;
        if !task.phase.terminal() {
            return Err(Error::State("avatar task is not terminal"));
        }
        let file = self.file(&task);
        match std::fs::symlink_metadata(&file) {
            Ok(_) => {
                Self::verify_file(&task, &file)?;
                std::fs::remove_file(file)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        task.source.clear();
        task.encoded.clear();
        self.save(token, &task)?;
        self.view(token, &task)
    }

    async fn detail(&self, token: &SessionToken) -> Result<SelfProfileView> {
        AccountProfile::new(self.session.clone())
            .detail_for(token)
            .await
    }
}
