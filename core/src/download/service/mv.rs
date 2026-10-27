//! M/334 and M/754: independent MV resolution/rights and MP4 download flow.
//! No point spending or automatic collection is hidden in a download action.
use super::{control::Control, *};
use crate::{
    Error, Result,
    download::transfer::{MediaUrl, TransferJob},
    membership::Quality,
    mv::{MvApi, MvDetail, MvPurpose},
    protocol::api::identifier,
};
use serde_json::{Value, json};

fn options() -> DownloadOptions {
    DownloadOptions {
        quality: Quality::Standard,
        tags: false,
        cover: false,
        lyrics: false,
        container: OutputContainer::Audio,
    }
}
fn choice(detail: &MvDetail, resolution: u32) -> Result<()> {
    let p = detail
        .privilege
        .as_ref()
        .ok_or(Error::Response("MV download privilege missing"))?;
    if p.unauthorized || p.fee.is_some_and(|fee| fee > 0) && p.paid.unwrap_or(0) == 0 {
        return Err(Error::State("MV download requires a separate entitlement"));
    }
    let selected = detail
        .download_options()
        .into_iter()
        .find(|v| v.br == resolution)
        .ok_or(Error::State("MV download resolution is unavailable"))?;
    if selected.point.unwrap_or(0) > 0 {
        return Err(Error::State("MV download requires explicit point spending"));
    }
    Ok(())
}
fn check_options(task: &DownloadTask) -> Result<()> {
    if task.options.tags
        || task.options.cover
        || task.options.lyrics
        || task.options.container != OutputContainer::Audio
        || task.encrypted
    {
        return Err(Error::State("MV download options were changed"));
    }
    Ok(())
}
impl DownloadService {
    async fn mv_detail(&self, id: &Value, resolution: u32) -> Result<MvDetail> {
        if resolution == 0 || resolution > 8640 {
            return Err(Error::InvalidInput("invalid MV resolution"));
        }
        let id = identifier(id)?;
        let token = self.session.token()?;
        self.guard(&token)?;
        let detail = self
            .session
            .run_for(&token, |api, _| async move {
                MvApi::new(api).download_options(&json!(id)).await
            })
            .await?;
        self.guard(&token)?;
        choice(&detail, resolution)?;
        Ok(detail)
    }
    pub async fn create_mv(
        &self,
        id: &Value,
        resolution: u32,
        destination: DownloadDestination,
    ) -> Result<DownloadTask> {
        let detail = self.mv_detail(id, resolution).await?;
        let _directory = crate::download::directory::gate::lock(&self.store, &self.owner)?;
        self.create_inner(
            &json!(detail.id),
            destination,
            options(),
            DownloadOrigin::Mv { resolution },
            Some(detail.naming_metadata()),
        )
    }
    pub async fn create_mv_default(&self, id: &Value, resolution: u32) -> Result<DownloadTask> {
        let detail = self.mv_detail(id, resolution).await?;
        self.create_default_from(
            &json!(detail.id),
            options(),
            DownloadOrigin::Mv { resolution },
            Some(detail.naming_metadata()),
        )
    }
    pub(super) async fn authorize_mv(
        &self,
        task: &mut DownloadTask,
        gate: &Control,
    ) -> Result<Value> {
        check_options(task)?;
        let DownloadOrigin::Mv { resolution } = task.origin else {
            return Err(Error::State("MV origin missing"));
        };
        let id = task.song_id.clone();
        let (detail, mut resource) = self
            .session
            .run_for(&gate.token, |api, _| async move {
                let api = MvApi::new(api);
                let detail = api.download_options(&json!(id)).await?;
                choice(&detail, resolution)?;
                let resource = api
                    .authorize(&json!(id), resolution, MvPurpose::Download)
                    .await?;
                Ok((detail, resource))
            })
            .await?;
        gate.require()?;
        // Bitrate option size is an estimate; only authorized size is definitive.
        // Empty MD5 is common for MV and means no digest was provided.
        if resource["md5"] == "" {
            resource.as_object_mut().expect("validated").remove("md5");
        } else if let Some(md5) = resource["md5"].as_str() {
            resource["md5"] = json!(md5.to_ascii_lowercase());
        }
        resource["origin"] = json!("mv");
        resource["type"] = json!("mp4");
        resource["requested_resolution"] = json!(resolution);
        resource["id"] = json!(task.song_id);
        task.song = Some(detail.naming_metadata());
        Ok(resource)
    }
    pub(super) fn resume_mv(
        &self,
        task: &mut DownloadTask,
        raw: &TransferJob,
        gate: &Control,
    ) -> Result<()> {
        gate.require()?;
        check_options(task)?;
        let DownloadOrigin::Mv { resolution } = task.origin else {
            return Err(Error::State("MV origin missing"));
        };
        let resource = task
            .resource
            .as_ref()
            .ok_or(Error::State("MV authorization missing"))?;
        if resource["origin"] != "mv"
            || resource["type"] != "mp4"
            || resource["id"] != task.song_id
            || resource["requested_resolution"] != resolution
            || raw.resource_id.as_ref() != Some(&task.song_id)
            || resource["size"].as_u64().filter(|v| *v > 0) != raw.resource.size
            || resource["md5"].as_str() != raw.resource.md5.as_deref()
            || resource["url"]
                .as_str()
                .map(MediaUrl::parse)
                .transpose()?
                .as_ref()
                != Some(&raw.resource.url)
        {
            return Err(Error::State("MV authorization changed before resume"));
        }
        task.status = DownloadStatus::Downloading;
        self.save_current(&gate.token, task)
    }
}
