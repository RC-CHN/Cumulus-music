//! The player observes local download state in the song or voice namespace.
use super::MusicClient;
use crate::{
    Error, Result,
    protocol::api::identifier,
    storage::{PlaybackDownloadKind, StateStore},
};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SongDownloadState {
    None,
    Failed,
    Paused,
    Complete,
    Waiting,
    Working,
}

pub(super) fn download_state(
    store: &StateStore,
    owner: &str,
    id: &str,
    kind: PlaybackDownloadKind,
) -> Result<SongDownloadState> {
    Ok(store
        .playback_download_rows(owner, id, kind)?
        .into_iter()
        .map(|row| match row.status.as_str() {
            "queued" => SongDownloadState::Waiting,
            "authorizing" | "downloading" | "processing" | "cancelling" => {
                SongDownloadState::Working
            }
            "complete" if row.moving => SongDownloadState::Working,
            "complete"
                if row.path.is_some_and(|path| {
                    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
                }) =>
            {
                SongDownloadState::Complete
            }
            "paused" => SongDownloadState::Paused,
            "error" => SongDownloadState::Failed,
            _ => SongDownloadState::None,
        })
        .max()
        .unwrap_or(SongDownloadState::None))
}

impl MusicClient {
    pub(in crate::client) async fn completed_voice_directory(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<std::path::PathBuf> {
        let context = self.downloads.inner.context(epoch)?;
        let id = identifier(&json!(id))?;
        tokio::task::spawn_blocking(move || {
            let mut found = None;
            for row in context.store.playback_download_rows(
                &context.owner,
                &id,
                PlaybackDownloadKind::Voice,
            )? {
                if row.status != "complete" || row.moving {
                    continue;
                }
                let Ok(path) =
                    context
                        .service
                        .completed_voice_directory_for(&context.token, &row.id, &id)
                else {
                    continue;
                };
                if found.as_ref().is_some_and(|previous| previous != &path) {
                    return Err(Error::State("program has multiple download directories"));
                }
                found = Some(path);
            }
            context.guard()?;
            found.ok_or(Error::DownloadedFileMissing)
        })
        .await
        .map_err(|_| Error::State("program download directory worker stopped"))?
    }

    pub(in crate::client) async fn completed_voice_file_candidate(
        &self,
        epoch: &str,
        voice: &str,
        media: &str,
    ) -> Result<(String, std::path::PathBuf)> {
        let context = self.downloads.inner.context(epoch)?;
        let voice = identifier(&json!(voice))?;
        let media = identifier(&json!(media))?;
        tokio::task::spawn_blocking(move || {
            let mut found = None;
            for row in context.store.playback_download_rows(
                &context.owner,
                &voice,
                PlaybackDownloadKind::Voice,
            )? {
                if row.status != "complete" || row.moving {
                    continue;
                }
                let Ok(path) = context
                    .files
                    .voice_reveal_candidate(&row.id, &voice, &media)
                else {
                    continue;
                };
                if found.is_some() {
                    return Err(Error::State("program has multiple completed files"));
                }
                found = Some((row.id, path));
            }
            context.guard()?;
            found.ok_or(Error::DownloadedFileMissing)
        })
        .await
        .map_err(|_| Error::State("program download file worker stopped"))?
    }

    pub async fn song_download_state(&self, epoch: &str, id: &str) -> Result<SongDownloadState> {
        self.playback_download_state(epoch, id, PlaybackDownloadKind::Song)
            .await
    }

    pub async fn voice_download_state(&self, epoch: &str, id: &str) -> Result<SongDownloadState> {
        self.playback_download_state(epoch, id, PlaybackDownloadKind::Voice)
            .await
    }

    async fn playback_download_state(
        &self,
        epoch: &str,
        id: &str,
        kind: PlaybackDownloadKind,
    ) -> Result<SongDownloadState> {
        let id = identifier(&json!(id))?;
        let session = self.session.clone();
        let token = session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let store = self.downloads.inner.store.clone();
        tokio::task::spawn_blocking(move || {
            let state = download_state(&store, &owner, &id, kind)?;
            session.with_token(&token, |_| Ok(state))
        })
        .await
        .map_err(|_| Error::State("playback download state worker stopped"))?
    }
}
