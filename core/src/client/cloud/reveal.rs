//! Native-only file selection for an ordinary cloud row.
use super::{
    Context, MusicClient,
    files::{regular, require_revision},
};
use crate::{
    Error, Result,
    cloud::upload::{CloudPhase, CloudUploadSourceReveal},
    download::{
        completed::{DownloadFileReveal, fs as verified},
        file_identity::{Identity, identity},
    },
    library::{LibraryItem, LocalLibrary, LocalRevision},
};
use std::{path::Path, sync::Arc};

/// This receipt binds the cloud snapshot and one verified native source.
/// Neither its path nor the receipt is returned to the renderer.
pub struct CloudFileReveal {
    context: Arc<Context>,
    revision: String,
    source: Source,
}

enum Source {
    Upload(Box<CloudUploadSourceReveal>),
    Download(Box<DownloadFileReveal>),
    Mapped(Box<MappedFile>),
}

struct MappedFile {
    library: LocalLibrary,
    catalog_id: String,
    item: LibraryItem,
    file: Identity,
    parent: Identity,
}

impl CloudFileReveal {
    fn issue(context: Arc<Context>, revision: &str, source: Source) -> Result<Self> {
        require_revision(&context, revision)?;
        Ok(Self {
            context,
            revision: revision.into(),
            source,
        })
    }

    pub fn path(&self) -> &Path {
        match &self.source {
            Source::Upload(reveal) => reveal.path(),
            Source::Download(reveal) => reveal.path(),
            Source::Mapped(reveal) => &reveal.item.path,
        }
    }

    pub fn check_current(&self) -> Result<()> {
        match &self.source {
            Source::Upload(reveal) => reveal.check_current()?,
            Source::Download(reveal) => reveal.check_current()?,
            Source::Mapped(reveal) => reveal.check_current(&self.context)?,
        }
        require_revision(&self.context, &self.revision)
    }
}

impl MappedFile {
    fn new(library: LocalLibrary, catalog_id: String, item: LibraryItem) -> Result<Self> {
        let file = identity(&item.path)?;
        let parent = identity(item.path.parent().ok_or(Error::QueueChanged)?)?;
        Ok(Self {
            library,
            catalog_id,
            item,
            file,
            parent,
        })
    }

    fn check_current(&self, context: &Context) -> Result<()> {
        self.library.check_mapped_file(
            &context.session,
            &context.token,
            &self.catalog_id,
            &self.item,
        )?;
        let path = &self.item.path;
        let bytes = match &self.item.revision {
            LocalRevision::File { fingerprint } => fingerprint.bytes,
            LocalRevision::Cue {
                sheet,
                sheet_fingerprint,
                source_fingerprint,
                ..
            } => {
                if !regular(sheet, sheet_fingerprint.bytes) || sheet.canonicalize()? != *sheet {
                    return Err(Error::QueueChanged);
                }
                source_fingerprint.bytes
            }
        };
        verified::parent(path, &self.parent)?;
        if !regular(path, bytes) || path.canonicalize()? != *path || identity(path)? != self.file {
            return Err(Error::QueueChanged);
        }
        context.guard()
    }
}

impl MusicClient {
    /// Try the established upload, download, and mapped-file candidates in
    /// order. Once chosen, a receipt can only recheck its own file.
    pub async fn cloud_file_reveal(
        &self,
        epoch: &str,
        song_id: &str,
        revision: &str,
    ) -> Result<CloudFileReveal> {
        let (context, _, catalog_id) = self
            .cloud_resource_context(epoch, song_id, revision)
            .await?;
        let upload = context.clone();
        let cloud_id = song_id.to_owned();
        let uploaded = tokio::task::spawn_blocking(move || {
            let result = upload
                .uploads
                .list()?
                .into_iter()
                .filter(|task| {
                    task.song_id.as_deref() == Some(cloud_id.as_str())
                        && matches!(task.phase, CloudPhase::Published | CloudPhase::Complete)
                })
                .find_map(|task| {
                    upload
                        .uploads
                        .published_source_reveal(&task.id, &cloud_id)
                        .ok()
                });
            upload.guard()?;
            Ok::<_, Error>(result)
        })
        .await
        .map_err(|_| Error::State("cloud resource worker stopped"))?;
        context.guard()?;
        let mut fault = uploaded.is_err();
        if let Ok(Some(reveal)) = uploaded {
            return CloudFileReveal::issue(context, revision, Source::Upload(Box::new(reveal)));
        }

        match self
            .cloud_file_downloads(epoch, song_id.into(), catalog_id.clone())
            .await
        {
            Ok(tasks) => {
                for task in tasks {
                    let reveal = self.download_file_reveal(epoch, task.id).await;
                    context.guard()?;
                    if let Ok(reveal) = reveal
                        && reveal.matches_cloud_association(song_id, catalog_id.as_deref())
                    {
                        return CloudFileReveal::issue(
                            context,
                            revision,
                            Source::Download(Box::new(reveal)),
                        );
                    }
                }
            }
            Err(_) => fault = true,
        }

        if let Some(catalog_id) = catalog_id {
            match self.local.context().await {
                Ok(local) => {
                    let scan = context.clone();
                    let scanned = tokio::task::spawn_blocking(move || {
                        let files = local.library.files_for_online_song(
                            &scan.session,
                            &scan.token,
                            &catalog_id,
                        )?;
                        let selected = files.into_iter().find_map(|item| {
                            let reveal =
                                MappedFile::new(local.library.clone(), catalog_id.clone(), item)
                                    .ok()?;
                            reveal.check_current(&scan).ok()?;
                            Some(reveal)
                        });
                        scan.guard()?;
                        Ok::<_, Error>(selected)
                    })
                    .await
                    .map_err(|_| Error::State("cloud scanned file worker stopped"))?;
                    context.guard()?;
                    fault |= scanned.is_err();
                    if let Ok(Some(reveal)) = scanned {
                        return CloudFileReveal::issue(
                            context,
                            revision,
                            Source::Mapped(Box::new(reveal)),
                        );
                    }
                }
                Err(_) => fault = true,
            }
        }

        require_revision(&context, revision)?;
        Err(Error::State(if fault {
            "cloud file associations unavailable"
        } else {
            "cloud local file missing or changed"
        }))
    }
}
