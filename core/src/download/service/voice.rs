//! Program downloads share transfer/publication/file recovery with songs, but
//! always re-check the program-specific entitlement and current main song.
use super::{control::Control, *};
use crate::{
    Error, Result,
    podcast::{Episode, PodcastApi},
};
use serde_json::{Value, json};

fn options_valid(options: &DownloadOptions) -> Result<()> {
    if options.container != OutputContainer::Audio {
        return Err(Error::InvalidInput(
            "program NCM container contract has not been recovered",
        ));
    }
    if options.lyrics {
        return Err(Error::InvalidInput(
            "program transcript download is not supported",
        ));
    }
    Ok(())
}
impl DownloadService {
    /// Expose only the original checked directory of an exact completed program.
    pub(crate) fn completed_voice_directory_for(
        &self,
        token: &crate::account::SessionToken,
        task_id: &str,
        voice: &str,
    ) -> Result<std::path::PathBuf> {
        let task = self.get_for(token, task_id)?;
        if task.status != DownloadStatus::Complete
            || task.file_operation.is_some()
            || !matches!(&task.origin, DownloadOrigin::Voice { voice_id } if voice_id == voice)
        {
            return Err(Error::QueueChanged);
        }
        let path = files::target(&task)?;
        let parent = path
            .parent()
            .ok_or(Error::State("download target has no parent"))?;
        let output = task
            .output_integrity
            .as_ref()
            .ok_or(Error::DownloadedFileMissing)?;
        let file = std::fs::symlink_metadata(path)?;
        if !file.is_file() || file.len() != output.bytes || parent.canonicalize()? != parent {
            return Err(Error::DownloadedFileMissing);
        }
        // Recheck the durable row after touching the filesystem, since a file
        // operation may have started while the folder was being verified.
        let live = self.get_for(token, task_id)?;
        if live.origin != task.origin
            || live.status != task.status
            || live.target != task.target
            || live.output_integrity != task.output_integrity
            || live.file_operation.is_some()
        {
            return Err(Error::QueueChanged);
        }
        Ok(parent.into())
    }

    async fn voice_detail(&self, id: &Value) -> Result<Episode> {
        let token = self.session.token()?;
        self.guard(&token)?;
        let item = self
            .session
            .run_for(&token, |api, _| async move {
                PodcastApi::new(api).episode(id).await
            })
            .await?;
        self.guard(&token)?;
        item.media_id()?;
        Ok(item)
    }
    pub async fn create_voice(
        &self,
        id: &Value,
        destination: DownloadDestination,
        options: DownloadOptions,
    ) -> Result<DownloadTask> {
        options_valid(&options)?;
        let item = self.voice_detail(id).await?;
        let _directory = crate::download::directory::gate::lock(&self.store, &self.owner)?;
        self.create_inner(
            &json!(item.media_id()?),
            destination,
            options,
            DownloadOrigin::Voice {
                voice_id: item.id.clone(),
            },
            Some(item.download_metadata()?),
        )
    }
    pub async fn create_voice_default(
        &self,
        id: &Value,
        options: DownloadOptions,
    ) -> Result<DownloadTask> {
        options_valid(&options)?;
        let item = self.voice_detail(id).await?;
        self.create_voice_default_from(item, options)
    }

    pub(crate) async fn create_scoped_voice_default(
        &self,
        id: &Value,
        options: DownloadOptions,
        lock_source: impl FnOnce() -> Result<crate::storage::OperationLock>,
    ) -> Result<DownloadTask> {
        options_valid(&options)?;
        let item = self.voice_detail(id).await?;
        // Do not hold the purchase cache lock over network I/O. Resolve the
        // exact source again afterwards and serialize it with native commit.
        let _source = lock_source()?;
        self.create_voice_default_from(item, options)
    }

    pub(crate) async fn create_visit_scoped_voice_default(
        &self,
        id: &Value,
        options: DownloadOptions,
        scope: &dyn ScopedTaskAdmission,
    ) -> Result<DownloadTask> {
        options_valid(&options)?;
        let item = self.voice_detail(id).await?;
        if item.display_status_withdrawn() || item.raw["grayDisplay"] == true {
            return Err(Error::QueueChanged);
        }
        self.create_scoped_voice_from(
            &json!(item.media_id()?),
            options,
            item.id.clone(),
            item.download_metadata()?,
            scope,
        )
    }

    fn create_voice_default_from(
        &self,
        item: Episode,
        options: DownloadOptions,
    ) -> Result<DownloadTask> {
        self.create_default_from(
            &json!(item.media_id()?),
            options,
            DownloadOrigin::Voice {
                voice_id: item.id.clone(),
            },
            Some(item.download_metadata()?),
        )
    }
    pub(super) async fn authorize_voice(
        &self,
        task: &mut DownloadTask,
        gate: &Control,
    ) -> Result<Value> {
        options_valid(&task.options)?;
        let DownloadOrigin::Voice { voice_id } = &task.origin else {
            return Err(Error::State("program download origin missing"));
        };
        let id = voice_id.clone();
        let song_id = task.song_id.clone();
        let level = task.options.quality;
        let (item, resource) = self
            .session
            .run_for(&gate.token, |api, _| async move {
                let api = PodcastApi::new(api);
                let item = api.episode(&json!(id)).await?;
                if item.media_id()? != song_id {
                    return Err(Error::Response("download program main song changed"));
                }
                let resource = api.download_url(&item, level.as_str()).await?;
                Ok((item, resource))
            })
            .await?;
        gate.require()?;
        task.song = Some(item.download_metadata()?);
        Ok(resource)
    }
}
