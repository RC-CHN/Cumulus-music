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
fn local_file(
    context: &Context,
    song_id: &str,
    downloads: Vec<DownloadTask>,
    verify: bool,
) -> Result<Option<PathBuf>> {
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
    for task in downloads {
        let (Some(path), Some(expected)) = (task.target, task.output_integrity) else {
            continue;
        };
        if regular(&path, expected.bytes)
            && (!verify || inspect_file(&path).is_ok_and(|actual| actual == expected))
        {
            context.guard()?;
            return Ok(Some(path));
        }
    }
    context.guard()?;
    Ok(None)
}
impl MusicClient {
    async fn cloud_resource_context(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<(Arc<Context>, CloudTrackView, Vec<DownloadTask>)> {
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
        let downloads = self
            .cloud_file_downloads(epoch, song_id.into(), track.public_song_id.clone())
            .await?;
        context.guard()?;
        Ok((context, track, downloads))
    }
    pub async fn cloud_resource(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<CloudResourceView> {
        let (context, track, downloads) = self
            .cloud_resource_context(epoch, song_id, revision)
            .await?;
        let epoch = epoch.to_owned();
        let revision = revision.to_owned();
        tokio::task::spawn_blocking(move || {
            let local_available =
                local_file(&context, &track.track.id, downloads, false)?.is_some();
            require_revision(&context, &revision)?;
            Ok(CloudResourceView {
                epoch,
                revision,
                track,
                local_available,
            })
        })
        .await
        .map_err(|_| Error::State("cloud resource worker stopped"))?
    }
    /// Native desktop use only: resolve again after the menu action and verify
    /// recorded content, membership, snapshot and account before returning.
    pub async fn cloud_directory_path(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<PathBuf> {
        let (context, track, downloads) = self
            .cloud_resource_context(epoch, song_id, revision)
            .await?;
        let revision = revision.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = local_file(&context, &track.track.id, downloads, true)?
                .ok_or(Error::State("cloud local file missing or changed"))?;
            let directory = file
                .parent()
                .ok_or(Error::State("cloud local file lacks directory"))?
                .to_owned();
            require_revision(&context, &revision)?;
            Ok(directory)
        })
        .await
        .map_err(|_| Error::State("cloud directory worker stopped"))?
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
