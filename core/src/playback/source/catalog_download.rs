//! Exact already-published song selection for an explicit catalog search row.
use super::{ResolvedSource, SourceResolver};
use crate::{Error, Result, account::SessionToken, download::service::DownloadService};
use serde_json::{Value, json};
use std::sync::Arc;

impl SourceResolver {
    pub(crate) async fn search_catalog_download(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Option<Arc<ResolvedSource>>> {
        let Some(song) = resource["id"].as_str() else {
            return Ok(None);
        };
        let candidate = self.published_search_song(song, token).await;
        // Even missing/corrupt publications cannot permit an online load for
        // a replaced account. Other readers on this source still check the token.
        self.session.check_generation(token)?;
        match candidate {
            Ok(value) => Ok(value),
            Err(error) => match error.root() {
                Error::Io(_)
                | Error::Media(_)
                | Error::Json(_)
                | Error::Storage(_)
                | Error::Response(_)
                | Error::State(_)
                | Error::InvalidInput(_)
                | Error::MutationBusy
                | Error::DownloadedFileMissing => Ok(None),
                _ => Err(error),
            },
        }
    }

    async fn published_search_song(
        &self,
        song: &str,
        token: &SessionToken,
    ) -> Result<Option<Arc<ResolvedSource>>> {
        let session = self.session.clone();
        let account = token.clone();
        let id = song.to_owned();
        let task = tokio::task::spawn_blocking(move || {
            DownloadService::published_catalog_song(&session, &account, &id)
        })
        .await
        .map_err(|_| Error::State("catalog download lookup worker stopped"))??;
        // No guess based on filename, mtime, quality or a different resource.
        let Some(task) = task else {
            return Ok(None);
        };
        if task.output_integrity.is_none() {
            return Ok(None);
        }
        let path = task.playback_target()?;
        let Some(path) = path.to_str() else {
            return Ok(None);
        };
        self.local(
            json!({"id": song, "download_task_id": task.id, "path": path}),
            token,
            Some(song),
        )
        .await
        .map(Some)
    }
}
