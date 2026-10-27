//! Official M/754/M/4 file actions with durable community-host recovery.
//! No remote API calls. All filesystem work is blocking: hosts use a worker.
mod commit;
mod execute;
pub(in crate::download) mod fs;
mod prepare;
mod types;
mod working;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::service::{DownloadTask, TASKS},
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use std::sync::Arc;
pub use types::{
    FileAction, FileEntryView, FileEvent, FileObserver, FileOperationView, FileOptions, FilePhase,
    FileStage,
};
use types::{ManagedFile, Operation};
pub(super) const JOURNAL: &str = "download_file_operations_v1";

#[derive(Clone)]
pub struct CompletedDownloads {
    pub(in crate::download) store: Arc<StateStore>,
    pub(in crate::download) owner: String,
    pub(in crate::download) session: Option<Arc<AccountSession>>,
    options: FileOptions,
    bound_token: Option<SessionToken>,
    migration: Option<(String, Option<SessionToken>)>,
}
impl CompletedDownloads {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        Ok(Self {
            store: session.persistent_store()?,
            owner,
            session: Some(session),
            bound_token: Some(token),
            options: Default::default(),
            migration: None,
        })
    }
    /// Explicit local ownership; this does not authenticate or load credentials.
    pub fn offline(store: Arc<StateStore>, owner: &str) -> Result<Self> {
        Ok(Self {
            store,
            owner: identifier(&serde_json::json!(owner))?,
            session: None,
            bound_token: None,
            options: Default::default(),
            migration: None,
        })
    }
    pub(in crate::download) fn for_migration(
        mut self,
        id: &str,
        token: Option<SessionToken>,
    ) -> Self {
        self.migration = Some((id.into(), token));
        self
    }
    pub(in crate::download) fn has_operation(&self, id: &str) -> Result<bool> {
        Ok(self.store.get(&self.owner, JOURNAL, id)?.is_some())
    }
    pub fn with_options(mut self, options: FileOptions) -> Self {
        self.options = options;
        self
    }
    pub(in crate::download) fn token(&self) -> Result<Option<SessionToken>> {
        if let Some((_, token)) = &self.migration {
            return Ok(token.clone());
        }
        Ok(self.bound_token.clone())
    }
    pub(in crate::download) fn guarded<T>(
        &self,
        token: &Option<SessionToken>,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        match (&self.session, token) {
            (Some(s), Some(t)) => s.with_token(t, |uid| {
                if uid == self.owner {
                    action()
                } else {
                    Err(Error::StaleResponse)
                }
            }),
            (None, None) => action(),
            _ => Err(Error::StaleResponse),
        }
    }
    fn guard(&self, t: &Option<SessionToken>) -> Result<()> {
        self.guarded(t, || {
            crate::download::directory::gate::require(
                &self.store,
                &self.owner,
                self.migration.as_ref().map(|m| m.0.as_str()),
            )
        })
    }
    fn locks(&self, task: &str) -> Result<Vec<OperationLock>> {
        valid_id(task)?;
        Ok(vec![
            OperationLock::acquire(self.store.directory(), "download-files", "all")?,
            OperationLock::acquire(
                self.store.directory(),
                "download-task",
                &serde_json::to_string(&(&self.owner, task))?,
            )?,
            OperationLock::acquire(self.store.directory(), "local", "library-index")?,
            OperationLock::acquire(
                self.store.directory(),
                "media-transfer",
                &serde_json::to_string(&(&self.owner, task))?,
            )?,
        ])
    }
    pub(in crate::download) fn task(&self, id: &str) -> Result<DownloadTask> {
        valid_id(id)?;
        let task: DownloadTask = serde_json::from_value(
            self.store
                .get(&self.owner, TASKS, id)?
                .ok_or(Error::State("download does not belong to this account"))?,
        )?;
        if task.id != id {
            return Err(Error::State("download task identity mismatch"));
        }
        Ok(task)
    }
    fn read(&self, id: &str) -> Result<Operation> {
        valid_id(id)?;
        let op: Operation =
            serde_json::from_value(self.store.get(&self.owner, JOURNAL, id)?.ok_or(
                Error::State("file operation does not belong to this account"),
            )?)?;
        if op.id != id || op.task_id != op.task.id {
            return Err(Error::State("file operation identity mismatch"));
        }
        Ok(op)
    }
    fn save(&self, op: &Operation) -> Result<()> {
        self.store
            .put(&self.owner, JOURNAL, &op.id, &serde_json::to_value(op)?)
    }
    fn event(&self, stage: FileEvent, op: &Operation) -> Result<()> {
        if let Some(callback) = &self.options.observer {
            callback(stage, &op.view())?;
        }
        Ok(())
    }
    pub fn get(&self, id: &str) -> Result<FileOperationView> {
        let t = self.token()?;
        self.guarded(&t, || Ok(self.read(id)?.view()))
    }
    pub fn list(&self) -> Result<Vec<FileOperationView>> {
        let t = self.token()?;
        self.guarded(&t, || {
            self.store
                .values(&self.owner, JOURNAL)?
                .into_values()
                .map(|v| Ok(serde_json::from_value::<Operation>(v)?.view()))
                .collect()
        })
    }
}
fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid file operation or task ID"));
    }
    Ok(())
}
