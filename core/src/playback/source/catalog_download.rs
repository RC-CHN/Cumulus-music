//! Exact already-published song selection for an explicit catalog search row.
use super::{ResolvedSource, SourceResolver, loudness};
use crate::{
    Error, Result,
    account::SessionToken,
    download::{
        offline::{HeldSearchFile, OFFLINE_PLAYLIST_DOWNLOAD, PlaylistDownload, SearchPublication},
        service::DownloadService,
    },
    library::matching::MatchHint,
    playback::{
        QueueEntry,
        engine::{Cancellation, MediaInput},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

impl SourceResolver {
    /// Strict natural continuation: verify/open only the exact publication
    /// admitted by the original M59 scan. No online fallback is authorized.
    pub(crate) async fn hold_ordinary_eof_download(
        &self,
        token: &SessionToken,
        entry: QueueEntry,
        expected: PlaylistDownload,
        cancel: &Cancellation,
    ) -> Result<(Arc<ResolvedSource>, HeldSearchFile)> {
        let session = self.session.clone();
        let account = token.clone();
        let fence = cancel.clone();
        let held = tokio::task::spawn_blocking(move || {
            fence.check()?;
            let song = crate::protocol::api::identifier(&entry.resource["id"])?;
            let task = DownloadService::published_catalog_song(&session, &account, &song)?
                .ok_or(Error::QueueChanged)?;
            if task.id != expected.task_id {
                return Err(Error::QueueChanged);
            }
            let publication = SearchPublication::bind_ordinary_eof(
                session,
                account,
                &task,
                &song,
                expected,
                entry.clone(),
            )?;
            let mut file = publication.open(&fence)?;
            file.check_ordinary_eof_file(&entry)?;
            let (gain, invalid) = if let Some(gain) = publication.gain() {
                (gain.clone(), false)
            } else {
                match file.metadata_hint() {
                    Ok(MatchHint::Song(value)) => (value["volumeDelta"].clone(), false),
                    Ok(MatchHint::Absent) => (Value::Null, false),
                    Ok(MatchHint::Invalid) | Err(_) => (Value::Null, true),
                }
            };
            let info = loudness::with_gain(
                json!({"kind":"local","resource_info":{"level":publication.quality()}}),
                (gain, invalid),
            );
            fence.check()?;
            Ok((
                Arc::new(ResolvedSource {
                    input: MediaInput::SearchPublication(publication),
                    info,
                    expires_at: None,
                }),
                file,
            ))
        })
        .await
        .map_err(|_| Error::State("ordinary EOF download worker stopped"))??;
        self.session.check_generation(token)?;
        cancel.check()?;
        Ok(held)
    }

    /// This queue entry can use only its exact sealed publication. Unlike
    /// ordinary search, any lookup, rights, format or decoder failure is terminal.
    pub(super) async fn offline_playlist_download(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<Arc<ResolvedSource>> {
        let expected: PlaylistDownload =
            serde_json::from_value(resource[OFFLINE_PLAYLIST_DOWNLOAD].clone())?;
        let song = crate::protocol::api::identifier(&resource["id"])?;
        if resource["download_task_id"].as_str() != Some(expected.task_id.as_str())
            || resource["song"]["id"].as_str() != Some(song.as_str())
            || [
                "path",
                "download_ledger",
                "download_voice_id",
                "cloud_song_id",
                "voice_id",
                "local_id",
                "playback_constraint",
                "trial_mode",
            ]
            .iter()
            .any(|field| !resource[*field].is_null())
        {
            return Err(Error::InvalidInput(
                "invalid offline playlist download identity",
            ));
        }
        let session = self.session.clone();
        let account = token.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<_> {
            let task = DownloadService::published_catalog_song(&session, &account, &song)?.ok_or(
                Error::State("offline playlist download is no longer available"),
            )?;
            if task.id != expected.task_id {
                return Err(Error::State("offline playlist download task changed"));
            }
            let publication = SearchPublication::bind_playlist_download(
                session, account, &task, &song, expected,
            )?;
            let info = loudness::with_gain(
                json!({"kind":"local","resource_info":{"level":publication.quality()}}),
                (publication.gain().cloned().unwrap_or(Value::Null), false),
            );
            Ok(Arc::new(ResolvedSource {
                input: MediaInput::SearchPublication(publication),
                info,
                expires_at: None,
            }))
        })
        .await
        .map_err(|_| Error::State("offline playlist download worker stopped"))??;
        self.session.check_generation(token)?;
        Ok(result)
    }

    pub(crate) async fn search_catalog_download(
        &self,
        resource: &Value,
        token: &SessionToken,
        cancel: &Cancellation,
    ) -> Result<Option<(Arc<ResolvedSource>, HeldSearchFile)>> {
        let Some(song) = resource["id"].as_str() else {
            return Ok(None);
        };
        let session = self.session.clone();
        let account = token.clone();
        let song = song.to_owned();
        let fence = cancel.clone();
        let candidate = tokio::task::spawn_blocking(move || -> Result<_> {
            let task = DownloadService::published_catalog_song(&session, &account, &song)?;
            // No guess based on filename, mtime, quality or a different resource.
            let Some(task) = task else {
                return Ok(None);
            };
            let publication = SearchPublication::bind(session, account, &task, &song)?;
            let mut file = publication.open(&fence)?;
            let (gain, invalid) = if let Some(gain) = publication.gain() {
                (gain.clone(), false)
            } else {
                match file.metadata_hint() {
                    Ok(MatchHint::Song(value)) => (value["volumeDelta"].clone(), false),
                    Ok(MatchHint::Absent) => (Value::Null, false),
                    Ok(MatchHint::Invalid) | Err(_) => (Value::Null, true),
                }
            };
            let info = loudness::with_gain(
                json!({"kind":"local","resource_info":{"level":publication.quality()}}),
                (gain, invalid),
            );
            fence.check()?;
            Ok(Some((
                Arc::new(ResolvedSource {
                    input: MediaInput::SearchPublication(publication),
                    info,
                    expires_at: None,
                }),
                file,
            )))
        })
        .await
        .map_err(|_| Error::State("catalog download lookup worker stopped"))?;
        // Even missing/corrupt publications cannot permit an online load for
        // a replaced account. Unsupported formats have not reached a decoder.
        self.session.check_generation(token)?;
        cancel.check()?;
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
}
