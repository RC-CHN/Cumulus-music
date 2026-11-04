//! The player observes only the current catalogue song's local download state.
use super::MusicClient;
use crate::{Error, Result, protocol::api::identifier};
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

impl MusicClient {
    pub async fn song_download_state(&self, epoch: &str, id: &str) -> Result<SongDownloadState> {
        let id = identifier(&json!(id))?;
        let session = self.session.clone();
        let token = session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let store = self.downloads.inner.store.clone();
        tokio::task::spawn_blocking(move || {
            let state = store
                .song_download_rows(&owner, &id)?
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
                .unwrap_or(SongDownloadState::None);
            session.with_token(&token, |_| Ok(state))
        })
        .await
        .map_err(|_| Error::State("song download state worker stopped"))?
    }
}
