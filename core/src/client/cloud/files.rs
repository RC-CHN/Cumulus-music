//! Cloud menu local-file association. Paths remain on the native side.
use super::*;
use crate::{download::service::DownloadTask, media::integrity::inspect_file};
use std::path::{Path, PathBuf};

fn regular(path: &Path, bytes: u64) -> bool {
    path.is_absolute()
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == bytes)
        && path.ancestors().all(|ancestor| {
            std::fs::symlink_metadata(ancestor).is_ok_and(|m| !m.file_type().is_symlink())
        })
}
fn local_file(context: &Context, song_id: &str, verify: bool) -> Result<Option<PathBuf>> {
    context.guard()?;
    // Official viewFileLocation prefers its upload-source association.
    for upload in context.uploads.list()? {
        if upload.song_id.as_deref() == Some(song_id)
            && matches!(upload.phase, CloudPhase::Published | CloudPhase::Complete)
            && regular(&upload.source, upload.total)
            && (!verify
                || context
                    .uploads
                    .verified_source_file(&upload.id, song_id)
                    .is_ok())
        {
            context.guard()?;
            return Ok(Some(upload.source));
        }
    }
    context.guard()?;
    Ok(None)
}
fn downloaded_file(downloads: Vec<DownloadTask>, verify: bool) -> Option<PathBuf> {
    for task in downloads {
        let (Some(path), Some(expected)) = (task.target, task.output_integrity) else {
            continue;
        };
        if regular(&path, expected.bytes)
            && (!verify || inspect_file(&path).is_ok_and(|actual| actual == expected))
        {
            return Some(path);
        }
    }
    None
}
impl MusicClient {
    async fn cloud_resource_context(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<(Arc<Context>, CloudTrackView, Option<String>)> {
        crate::protocol::api::identifier(&serde_json::json!(song_id))?;
        let context = self.cloud.inner.context(epoch).await?;
        let snapshot = context.library.cached()?.ok_or(Error::QueueChanged)?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        let track = view::catalog(
            Some(&snapshot),
            &CloudQuery {
                tab: CloudTab::Uploaded,
                keyword: String::new(),
                sort: CloudSort::Natural,
                descending: false,
                offset: 0,
                limit: 200,
            },
        )?
        .into_iter()
        .find(|t| t.track.id == song_id)
        .ok_or(Error::QueueChanged)?;
        let raw = &snapshot
            .items
            .iter()
            .find(|item| item.song_id == song_id)
            .ok_or(Error::QueueChanged)?
            .song;
        let catalog_id = crate::protocol::api::identifier(&raw["_cloud_catalog_id"])
            .ok()
            .or_else(|| crate::protocol::api::identifier(&raw["_cloud_public_id"]).ok());
        context.guard()?;
        Ok((context, track, catalog_id))
    }
    pub async fn cloud_resource(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<CloudResourceView> {
        let (context, track, catalog_id) = self
            .cloud_resource_context(epoch, song_id, revision)
            .await?;
        let result = self
            .resolve_cloud_file(epoch, context.clone(), song_id, catalog_id, false)
            .await;
        // Local catalog/journal faults must not remove playback, collection or
        // delete actions. Account and cloud revision failures still fail closed.
        require_revision(&context, revision)?;
        let (local_available, local_issue) = match result {
            Ok(file) => (file.is_some(), None),
            Err(_) => (false, Some("本地文件关联暂不可用，其他歌曲操作仍可使用")),
        };
        Ok(CloudResourceView {
            epoch: epoch.into(),
            revision: revision.into(),
            track,
            local_available,
            local_issue,
        })
    }

    /// Native desktop use only: resolve again after the menu action and verify
    /// recorded content, membership, snapshot and account before returning.
    pub async fn cloud_directory_path(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<PathBuf> {
        let (context, _, catalog_id) = self
            .cloud_resource_context(epoch, song_id, revision)
            .await?;
        let file = self
            .resolve_cloud_file(epoch, context.clone(), song_id, catalog_id, true)
            .await?
            .ok_or(Error::State("cloud local file missing or changed"))?;
        let directory = file
            .parent()
            .ok_or(Error::State("cloud local file lacks directory"))?
            .to_owned();
        require_revision(&context, revision)?;
        Ok(directory)
    }
    async fn resolve_cloud_file(
        &self,
        epoch: &str,
        context: Arc<Context>,
        song_id: &str,
        catalog_id: Option<String>,
        verify: bool,
    ) -> Result<Option<PathBuf>> {
        let upload_context = context.clone();
        let cloud_id = song_id.to_owned();
        let uploaded =
            tokio::task::spawn_blocking(move || local_file(&upload_context, &cloud_id, verify))
                .await
                .map_err(|_| Error::State("cloud resource worker stopped"))?;
        context.guard()?;
        let mut fault = uploaded.is_err();
        if let Ok(Some(path)) = uploaded {
            return Ok(Some(path));
        }
        match self
            .cloud_file_downloads(epoch, song_id.into(), catalog_id.clone())
            .await
        {
            Ok(downloads) => {
                let downloaded =
                    tokio::task::spawn_blocking(move || downloaded_file(downloads, verify))
                        .await
                        .map_err(|_| Error::State("cloud file worker stopped"))?;
                context.guard()?;
                if let Some(path) = downloaded {
                    return Ok(Some(path));
                }
            }
            Err(_) => fault = true,
        }
        if let Some(catalog_id) = catalog_id {
            match self.local.context().await {
                Ok(local) => {
                    let scan_context = context.clone();
                    let scanned: Result<Option<PathBuf>> = tokio::task::spawn_blocking(move || {
                        let files = local.library.files_for_online_song(
                            &scan_context.session,
                            &scan_context.token,
                            &catalog_id,
                        )?;
                        Ok(files.into_iter().find_map(|item| {
                            let bytes = match &item.revision {
                                crate::library::LocalRevision::File { fingerprint } => {
                                    fingerprint.bytes
                                }
                                crate::library::LocalRevision::Cue {
                                    source_fingerprint, ..
                                } => source_fingerprint.bytes,
                            };
                            regular(&item.path, bytes).then_some(item.path)
                        }))
                    })
                    .await
                    .map_err(|_| Error::State("cloud scanned file worker stopped"))?;
                    context.guard()?;
                    fault |= scanned.is_err();
                    if let Ok(Some(path)) = scanned {
                        return Ok(Some(path));
                    }
                }
                Err(_) => fault = true,
            }
        }
        context.guard()?;
        if fault {
            return Err(Error::State("cloud file associations unavailable"));
        }
        Ok(None)
    }
}
fn require_revision(context: &Context, revision: &str) -> Result<()> {
    context.guard()?;
    if context
        .library
        .cached()?
        .is_none_or(|snapshot| snapshot.revision != revision)
    {
        return Err(Error::QueueChanged);
    }
    Ok(())
}

impl MusicClient {
    pub async fn cloud_upload_directory_path(&self, epoch: &str, id: &str) -> Result<PathBuf> {
        let context = self.cloud.inner.context(epoch).await?;
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = context.uploads.verified_task_source_file(&id)?;
            if !regular(&file, context.uploads.get(&id)?.total) {
                return Err(Error::State("cloud upload source unavailable"));
            }
            let directory = file
                .parent()
                .ok_or(Error::State("cloud upload directory missing"))?
                .to_owned();
            context.guard()?;
            Ok(directory)
        })
        .await
        .map_err(|_| Error::State("cloud file worker stopped"))?
    }
}
