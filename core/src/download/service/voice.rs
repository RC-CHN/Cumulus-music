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
