use super::{MusicClient, dto::*, playlists::editor};
use crate::{
    Error, Result,
    account::SessionToken,
    cloud::nos::{NosBackend, NosRunOptions},
    playlist::{
        PlaylistService,
        cover::{CoverPhase, PlaylistCovers},
    },
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};

#[derive(Default)]
pub(super) struct CoverHost {
    backend: Option<Arc<dyn NosBackend>>,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoverCommand {
    Save { crop: CoverCrop },
    Continue,
    RetryAllocation,
    Reconcile,
    Rebase { revision: String },
    Pause,
    Renew,
    Abandon,
    Cleanup,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverOutcome {
    pub task: CoverTaskView,
    pub editor: Option<PlaylistEditor>,
    pub warning: Option<&'static str>,
}
impl MusicClient {
    /// Native dependency injection; no frontend command can replace transport.
    pub fn with_playlist_cover_backend(mut self, backend: Arc<dyn NosBackend>) -> Self {
        self.playlist_cover.backend = Some(backend);
        self
    }
    fn covers(&self, epoch: &str) -> Result<(SessionToken, Arc<PlaylistCovers>)> {
        let token = self.playlist_token(epoch)?;
        let covers = match &self.playlist_cover.backend {
            Some(backend) => {
                PlaylistCovers::with_backend(self.session.clone(), backend.clone(), 256 * 1024)?
            }
            None => PlaylistCovers::new(self.session.clone())?,
        };
        self.session.with_token(&token, |_| Ok(()))?;
        Ok((token, Arc::new(covers)))
    }
    pub fn validate_playlist_epoch(&self, epoch: &str) -> Result<()> {
        self.playlist_token(epoch).map(|_| ())
    }
    pub async fn playlist_cover_tasks(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<CoverTaskView>>> {
        let (token, covers) = self.covers(epoch)?;
        let t = token.clone();
        let value = tokio::task::spawn_blocking(move || covers.list(&t))
            .await
            .map_err(|_| Error::State("cover task worker stopped"))??;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn playlist_cover_image(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<CoverImage>> {
        let (token, covers) = self.covers(epoch)?;
        let t = token.clone();
        let id = id.to_owned();
        let value = tokio::task::spawn_blocking(move || covers.image(&t, &id))
            .await
            .map_err(|_| Error::State("cover preview worker stopped"))??;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    /// Only the native file picker supplies this path. The IPC never accepts
    /// one from the WebView, and neither source bytes nor paths are returned.
    pub async fn select_playlist_cover(
        &self,
        epoch: &str,
        playlist_id: &str,
        revision: &str,
        path: &Path,
    ) -> Result<AccountData<CoverTaskView>> {
        let (token, covers) = self.covers(epoch)?;
        let value = covers
            .select_file(&token, playlist_id, revision, path)
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn replace_playlist_cover(
        &self,
        epoch: &str,
        playlist_id: &str,
        revision: &str,
        id: &str,
        path: &Path,
    ) -> Result<AccountData<CoverTaskView>> {
        let (token, covers) = self.covers(epoch)?;
        let task = covers.get(&token, id)?;
        if task.playlist_id != playlist_id {
            return Err(Error::InvalidInput("cover belongs to another playlist"));
        }
        let store = self.session.persistent_store()?;
        let owner = self.session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let _action = OperationLock::acquire(
            store.directory(),
            "cover-client-action",
            &format!("{owner}:{id}"),
        )?;
        let value = covers.replace_file(&token, id, revision, path).await?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn playlist_cover_command(
        &self,
        epoch: &str,
        id: &str,
        command: CoverCommand,
    ) -> Result<AccountData<CoverOutcome>> {
        let (token, covers) = self.covers(epoch)?;
        let store = self.session.persistent_store()?;
        let owner = self.session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        // Serialize the whole prepare/upload/bind action, not just its pieces:
        // a second window cannot replace crop bytes between prepare and send.
        let _action = if matches!(command, CoverCommand::Pause) {
            None
        } else {
            Some(OperationLock::acquire(
                store.directory(),
                "cover-client-action",
                &format!("{owner}:{id}"),
            )?)
        };
        let mut task = match command {
            CoverCommand::Save { crop } => {
                let c = covers.clone();
                let t = token.clone();
                let key = id.to_owned();
                tokio::task::spawn_blocking(move || c.prepare(&t, &key, crop))
                    .await
                    .map_err(|_| Error::State("cover crop worker stopped"))??;
                advance(&covers, &token, id).await?
            }
            CoverCommand::Continue => advance(&covers, &token, id).await?,
            CoverCommand::RetryAllocation => {
                covers.retry_allocation(&token, id)?;
                advance(&covers, &token, id).await?
            }
            CoverCommand::Reconcile => covers.reconcile(&token, id).await?,
            CoverCommand::Rebase { revision } => covers.rebase(&token, id, &revision).await?,
            CoverCommand::Pause => covers.pause(&token, id)?,
            CoverCommand::Renew => covers.renew(&token, id).await?,
            CoverCommand::Abandon => covers.abandon(&token, id)?,
            CoverCommand::Cleanup => covers.cleanup(&token, id)?,
        };
        let mut warning = None;
        if task.phase.terminal() && task.cleanup_pending {
            let c = covers.clone();
            let t = token.clone();
            let key = id.to_owned();
            match tokio::task::spawn_blocking(move || c.cleanup(&t, &key)).await {
                Ok(Ok(cleaned)) => task = cleaned,
                _ => warning = Some("操作结果已保留，本地图片缓存清理失败，请重试清理"),
            }
        }
        let observed = if task.phase == CoverPhase::Confirmed {
            match PlaylistService::new(self.session.clone())?
                .cached_detail(&token, &task.playlist_id)
            {
                Ok(Some(p)) => Some(editor(p, &owner)),
                _ => {
                    warning = Some("封面已保存，歌单信息暂未刷新");
                    None
                }
            }
        } else {
            None
        };
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: CoverOutcome {
                task,
                editor: observed,
                warning,
            },
        })
    }
}
async fn advance(covers: &PlaylistCovers, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
    let mut task = covers.get(token, id)?;
    if matches!(task.phase, CoverPhase::Prepared | CoverPhase::Uploading) {
        task = covers.upload(token, id, NosRunOptions::default()).await?;
    }
    if task.phase == CoverPhase::Uploaded {
        covers.bind(token, id).await
    } else if task.phase == CoverPhase::Binding {
        covers.reconcile(token, id).await
    } else {
        Ok(task)
    }
}
