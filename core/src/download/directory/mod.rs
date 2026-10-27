//! Official M/3022 relative-path migration and M/3097 directory/space checks.
//! Cooperative quiescing and durable reverse-order rollback replace app restart.
pub(in crate::download) mod gate;
pub(in crate::download) mod paths;
mod plan;
mod run;
mod types;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{
        completed::{CompletedDownloads, FilePhase},
        file_identity::identity,
        service::{DownloadStatus, DownloadTask, TASKS},
    },
    storage::{OperationLock, StateStore},
};
use gate::{BATCHES, SETTINGS};
use std::{path::Path, sync::Arc};
pub use types::*;

#[derive(Clone)]
pub struct DownloadDirectory {
    files: CompletedDownloads,
    options: DirectoryOptions,
}
impl DownloadDirectory {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            files: CompletedDownloads::new(session)?,
            options: Default::default(),
        })
    }
    pub fn offline(store: Arc<StateStore>, owner: &str) -> Result<Self> {
        Ok(Self {
            files: CompletedDownloads::offline(store, owner)?,
            options: Default::default(),
        })
    }
    pub fn with_options(mut self, options: DirectoryOptions) -> Self {
        self.options = options;
        self
    }
    pub fn settings(&self) -> Result<Option<DirectorySettings>> {
        let token = self.files.token()?;
        self.files.guarded(&token, || self.read_settings())
    }
    fn read_settings(&self) -> Result<Option<DirectorySettings>> {
        self.files
            .store
            .get(&self.files.owner, SETTINGS, "settings")?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }
    pub fn active(&self) -> Result<Option<String>> {
        self.files.guarded(&self.files.token()?, || {
            gate::active(&self.files.store, &self.files.owner)
        })
    }
    pub fn configure(
        &self,
        root: &Path,
        path_type: u8,
        file_type: u8,
    ) -> Result<DirectorySettings> {
        if path_type > 2 || file_type > 2 {
            return Err(Error::InvalidInput("download layout must be 0..2"));
        }
        let token = self.files.token()?;
        let _lock = gate::lock(&self.files.store, &self.files.owner)?;
        self.files.guarded(&token, || {
            gate::require(&self.files.store, &self.files.owner, None)
        })?;
        let root = paths::root(root, self.files.store.directory(), true)?;
        paths::writable(&root)?;
        let result = DirectorySettings {
            root,
            path_type,
            file_type,
            revision: new_id(),
        };
        self.files.guarded(&token, || {
            self.files.store.put(
                &self.files.owner,
                SETTINGS,
                "settings",
                &serde_json::to_value(&result)?,
            )
        })?;
        Ok(result)
    }
    pub fn start(&self, target: &Path, source: Option<&Path>) -> Result<MigrationView> {
        let token = self.files.token()?;
        let _directory = gate::lock(&self.files.store, &self.files.owner)?;
        let _files = OperationLock::acquire(self.files.store.directory(), "download-files", "all")?;
        self.files.guarded(&token, || {
            gate::require(&self.files.store, &self.files.owner, None)
        })?;
        let before = self.read_settings()?;
        let source = source
            .or_else(|| before.as_ref().map(|s| s.root.as_path()))
            .ok_or(Error::InvalidInput(
                "configure or specify a source directory",
            ))?;
        let source = paths::root(source, self.files.store.directory(), false)?;
        if before.as_ref().is_some_and(|s| s.root != source) {
            return Err(Error::InvalidInput(
                "source must match configured directory",
            ));
        }
        let target = paths::root(target, self.files.store.directory(), true)?;
        if source.starts_with(&target) || target.starts_with(&source) {
            return Err(Error::InvalidInput(
                "migration roots must be distinct and not nested",
            ));
        }
        paths::writable(&target)?;
        let batch = self.files.guarded(&token, || {
            self.files.store.transaction(|tx| {
                let mut original = std::collections::BTreeMap::new();
                for row in tx.rows(TASKS)? {
                    if row.owner != self.files.owner {
                        continue;
                    }
                    let task: DownloadTask = serde_json::from_value(row.value)?;
                    if task.file_operation.is_some() {
                        return Err(Error::State(
                            "finish or cancel pending file operations first",
                        ));
                    }
                    original.insert(task.id, task.status);
                }
                let batch = types::Batch {
                    source_identity: identity(&source)?,
                    target_identity: identity(&target)?,
                    before,
                    original,
                    view: MigrationView {
                        id: new_id(),
                        source,
                        target,
                        phase: MigrationPhase::Quiescing,
                        items: vec![],
                        skipped: vec![],
                        waiting: vec![],
                        required_bytes: 0,
                        error: None,
                    },
                };
                tx.put(
                    &self.files.owner,
                    SETTINGS,
                    "active",
                    &serde_json::to_value(&batch.view.id)?,
                )?;
                tx.put(
                    &self.files.owner,
                    BATCHES,
                    &batch.view.id,
                    &serde_json::to_value(&batch)?,
                )?;
                Ok(batch)
            })
        })?;
        self.event(DirectoryEvent::Intent, &batch)?;
        self.guard(&batch, &token)?;
        Ok(batch.view)
    }
    pub fn get(&self, id: &str) -> Result<MigrationView> {
        self.files
            .guarded(&self.files.token()?, || Ok(self.read(id)?.view))
    }
    pub fn list(&self) -> Result<Vec<MigrationView>> {
        self.files.guarded(&self.files.token()?, || {
            self.files
                .store
                .values(&self.files.owner, BATCHES)?
                .into_values()
                .map(|v| Ok(serde_json::from_value::<types::Batch>(v)?.view))
                .collect()
        })
    }
    fn read(&self, id: &str) -> Result<types::Batch> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidInput("invalid migration ID"));
        }
        let batch: types::Batch = serde_json::from_value(
            self.files
                .store
                .get(&self.files.owner, BATCHES, id)?
                .ok_or(Error::State("migration does not belong to this account"))?,
        )?;
        if batch.view.id != id {
            return Err(Error::State("migration identity mismatch"));
        }
        Ok(batch)
    }
    fn save(&self, batch: &types::Batch, token: &Option<SessionToken>) -> Result<()> {
        self.files.guarded(token, || {
            self.files.store.put(
                &self.files.owner,
                BATCHES,
                &batch.view.id,
                &serde_json::to_value(batch)?,
            )
        })
    }
    fn guard(&self, batch: &types::Batch, token: &Option<SessionToken>) -> Result<()> {
        self.files.guarded(token, || {
            gate::require(&self.files.store, &self.files.owner, Some(&batch.view.id))
        })?;
        paths::check(&batch.view.source, &batch.source_identity)?;
        paths::check(&batch.view.target, &batch.target_identity)
    }
    fn event(&self, event: DirectoryEvent, batch: &types::Batch) -> Result<()> {
        if let Some(callback) = &self.options.observer {
            callback(event, &batch.view)?;
        }
        Ok(())
    }
    fn children(&self, batch: &types::Batch, token: &Option<SessionToken>) -> CompletedDownloads {
        self.files
            .clone()
            .with_options(self.options.files.clone())
            .for_migration(&batch.view.id, token.clone())
    }
}
fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
