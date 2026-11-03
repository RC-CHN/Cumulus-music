use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::{
        SessionToken,
        profile::avatar::{AvatarCrop, AvatarEditor, AvatarImage, AvatarPhase, AvatarTaskView},
    },
    cloud::nos::{NosBackend, NosRunOptions},
    storage::OperationLock,
};
use serde::Deserialize;
use std::{path::Path, sync::Arc};

#[derive(Default)]
pub(super) struct AvatarHost {
    backend: Option<Arc<dyn NosBackend>>,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AccountAvatarCommand {
    Save { crop: AvatarCrop },
    Continue,
    Reconcile,
    AcceptCurrent { expected_revision: String },
    Abandon,
    Cleanup,
}

impl MusicClient {
    pub fn with_account_avatar_backend(mut self, backend: Arc<dyn NosBackend>) -> Self {
        self.account_avatar.backend = Some(backend);
        self
    }

    pub fn validate_account_avatar_epoch(&self, epoch: &str) -> Result<()> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))
    }

    fn avatar_editor(&self, epoch: &str) -> Result<(SessionToken, Arc<AvatarEditor>)> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let editor = match &self.account_avatar.backend {
            Some(backend) => {
                AvatarEditor::with_backend(self.session.clone(), backend.clone(), 256 * 1024)?
            }
            None => AvatarEditor::new(self.session.clone())?,
        };
        Ok((token, Arc::new(editor)))
    }

    fn avatar_result<T>(
        &self,
        token: &SessionToken,
        epoch: &str,
        value: T,
    ) -> Result<AccountData<T>> {
        self.session.with_token(token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn account_avatar_tasks(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<AvatarTaskView>>> {
        let (token, editor) = self.avatar_editor(epoch)?;
        let owner = token.clone();
        let mut value = tokio::task::spawn_blocking(move || editor.list(&owner))
            .await
            .map_err(|_| Error::State("avatar list worker stopped"))??;
        if let Some(task) = value.iter_mut().find(|task| avatar_adopted(task.phase)) {
            task.metadata_pending = self.sync_avatar_summary(&token).is_err();
        }
        self.avatar_result(&token, epoch, value)
    }

    pub async fn account_avatar_image(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<AvatarImage>> {
        let (token, editor) = self.avatar_editor(epoch)?;
        let owner = token.clone();
        let id = id.to_owned();
        let value = tokio::task::spawn_blocking(move || editor.image(&owner, &id))
            .await
            .map_err(|_| Error::State("avatar preview worker stopped"))??;
        self.avatar_result(&token, epoch, value)
    }

    /// The path is supplied by a native host picker, never the renderer.
    pub async fn select_account_avatar(
        &self,
        epoch: &str,
        path: &Path,
    ) -> Result<AccountData<AvatarTaskView>> {
        let (token, editor) = self.avatar_editor(epoch)?;
        let _action = self.avatar_action(&token)?;
        let task = editor.select_file(&token, path).await?;
        self.avatar_result(&token, epoch, task)
    }

    pub async fn account_avatar_command(
        &self,
        epoch: &str,
        id: &str,
        command: AccountAvatarCommand,
    ) -> Result<AccountData<AvatarTaskView>> {
        let (token, editor) = self.avatar_editor(epoch)?;
        let _action = self.avatar_action(&token)?;
        let mut task = match command {
            AccountAvatarCommand::Save { crop } => {
                let worker = editor.clone();
                let worker_token = token.clone();
                let worker_id = id.to_owned();
                tokio::task::spawn_blocking(move || {
                    worker.prepare(&worker_token, &worker_id, crop)
                })
                .await
                .map_err(|_| Error::State("avatar crop worker stopped"))??;
                advance(&editor, &token, id).await?
            }
            AccountAvatarCommand::Continue => advance(&editor, &token, id).await?,
            AccountAvatarCommand::Reconcile => editor.reconcile(&token, id).await?,
            AccountAvatarCommand::AcceptCurrent { expected_revision } => {
                editor
                    .accept_current(&token, id, &expected_revision)
                    .await?
            }
            AccountAvatarCommand::Abandon => editor.abandon(&token, id)?,
            AccountAvatarCommand::Cleanup => editor.cleanup(&token, id)?,
        };
        if avatar_adopted(task.phase) {
            let metadata_pending = self.sync_avatar_summary(&token).is_err();
            if task.cleanup_pending {
                let worker_token = token.clone();
                let worker_id = id.to_owned();
                if let Ok(Ok(cleaned)) =
                    tokio::task::spawn_blocking(move || editor.cleanup(&worker_token, &worker_id))
                        .await
                {
                    task = cleaned;
                }
            }
            task.metadata_pending = metadata_pending;
        }
        self.avatar_result(&token, epoch, task)
    }

    fn avatar_action(&self, token: &SessionToken) -> Result<OperationLock> {
        let store = self.session.persistent_store()?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        OperationLock::acquire(store.directory(), "account-avatar-client-action", &owner)
    }

    fn sync_avatar_summary(&self, token: &SessionToken) -> Result<()> {
        if let Some(client) = self.native_account_client()? {
            self.registry
                .record_current_for(&self.session, &client, token)?;
        }
        Ok(())
    }
}

fn avatar_adopted(phase: AvatarPhase) -> bool {
    matches!(
        phase,
        AvatarPhase::Confirmed | AvatarPhase::AcceptedCurrentState
    )
}

async fn advance(editor: &AvatarEditor, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
    let mut task = editor.get(token, id)?;
    if matches!(task.phase, AvatarPhase::Prepared | AvatarPhase::Uploading) {
        task = editor.upload(token, id, NosRunOptions::default()).await?;
    }
    if task.phase == AvatarPhase::Uploaded {
        return editor.bind(token, id).await;
    }
    Ok(task)
}
