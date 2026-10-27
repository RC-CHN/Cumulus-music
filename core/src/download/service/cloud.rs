//! M/754 internalDownload's cloud branch and M/334 -> M/15 cloud/dowonload.
use super::{control::Control, *};
use crate::{
    Error, Result,
    cloud::{CloudApi, library::CloudEntry},
    protocol::api::identifier,
};
use serde_json::{Value, json};

impl DownloadService {
    pub(super) fn resume_cloud(
        &self,
        task: &mut DownloadTask,
        raw: &crate::download::transfer::TransferJob,
        gate: &Control,
    ) -> Result<()> {
        gate.require()?;
        let DownloadOrigin::Cloud { file_size, .. } = task.origin else {
            return Err(Error::State("cloud origin missing"));
        };
        let resource = task
            .resource
            .as_ref()
            .ok_or(Error::State("cloud authorization missing"))?;
        if task.options.container != OutputContainer::Audio
            || task.options.tags
            || task.options.cover
            || task.encrypted
            || resource["origin"] != "cloud"
            || resource["size"].as_u64() != Some(file_size)
            || resource["id"].as_str() != Some(&task.song_id)
            || raw.resource_id.as_ref() != Some(&task.song_id)
            || resource["url"]
                .as_str()
                .map(crate::download::transfer::MediaUrl::parse)
                .transpose()?
                .as_ref()
                != Some(&raw.resource.url)
            || raw.resource.size != Some(file_size)
        {
            return Err(Error::State("cloud original authorization changed"));
        }
        task.status = DownloadStatus::Downloading;
        self.save_current(&gate.token, task)
    }
    /// Queue a personal cloud original, preserving bytes and embedded metadata.
    /// Authorization and ownership are rechecked before contacting the media host.
    pub async fn create_cloud(
        &self,
        song_id: &Value,
        destination: DownloadDestination,
        lyrics: bool,
    ) -> Result<DownloadTask> {
        let id = identifier(song_id)?;
        let token = self.session.token()?;
        self.guard(&token)?;
        let entry = self
            .session
            .run_for(&token, |api, _| async move {
                CloudApi::new(api)
                    .snapshot()
                    .await?
                    .items
                    .into_iter()
                    .find(|v| v.song_id == id)
                    .ok_or(Error::State("cloud download is not in this account"))
            })
            .await?;
        self.guard(&token)?;
        let _directory = crate::download::directory::gate::lock(&self.store, &self.owner)?;
        let origin = origin(&entry)?;
        self.create_inner(
            &json!(entry.song_id),
            destination,
            options(lyrics),
            origin,
            Some(entry.song),
        )
    }
    /// The client boundary supplies entries from its own fresh, revision-checked
    /// complete snapshot. This is not an IPC accepting arbitrary song metadata.
    pub(crate) fn create_cloud_default(
        &self,
        entry: CloudEntry,
        lyrics: bool,
    ) -> Result<DownloadTask> {
        let origin = origin(&entry)?;
        self.create_default_from(
            &json!(entry.song_id),
            options(lyrics),
            origin,
            Some(entry.song),
        )
    }
    pub(super) async fn authorize_cloud(
        &self,
        task: &mut DownloadTask,
        gate: &Control,
    ) -> Result<Value> {
        let DownloadOrigin::Cloud {
            file_name,
            file_size,
        } = &task.origin
        else {
            return Err(Error::State("cloud origin missing"));
        };
        if task.options.container != OutputContainer::Audio
            || task.options.tags
            || task.options.cover
        {
            return Err(Error::State("cloud original options were changed"));
        }
        let id = task.song_id.clone();
        let (entry, response) = self
            .session
            .run_for(&gate.token, |api, _| async move {
                let cloud = CloudApi::new(api);
                let entry = cloud
                    .snapshot()
                    .await?
                    .items
                    .into_iter()
                    .find(|v| v.song_id == id)
                    .ok_or(Error::State("cloud download is no longer in this account"))?;
                if entry.file_name != *file_name || entry.file_size != *file_size {
                    return Err(Error::State("cloud original changed before authorization"));
                }
                let response = cloud.download(&json!(id)).await?;
                Ok((entry, response))
            })
            .await?;
        gate.require()?;
        if !response["freeTrialInfo"].is_null() {
            return Err(Error::Response("trial media is not a cloud original"));
        }
        if let Some(id) = response.get("songId").filter(|id| !id.is_null())
            && identifier(id)? != task.song_id
        {
            return Err(Error::Response(
                "cloud authorization belongs to another song",
            ));
        }
        let name = response["name"]
            .as_str()
            .ok_or(Error::Response("cloud download name missing"))?;
        // Exact ASCII suffix semantics of M/754: trim backslashes, quotes and
        // whitespace at both ends, then take a final dot + word characters.
        let name = name.trim_matches(|c: char| {
            matches!(c, '\\' | '\'' | '"')
                || crate::protocol::wire::serialization::is_js_whitespace(c)
        });
        let suffix = name
            .rsplit_once('.')
            .map(|(_, s)| s)
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 16
                    && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            })
            .map(str::to_owned);
        let size = response["size"]
            .as_u64()
            .filter(|s| *s > 0)
            .ok_or(Error::Response("cloud download size missing"))?;
        if size != *file_size {
            return Err(Error::Response(
                "cloud download size differs from account original",
            ));
        }
        let url = response["url"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::Response("cloud download URL missing"))?;
        crate::download::transfer::MediaUrl::parse(url)?;
        let suffix = match suffix {
            Some(suffix) => suffix,
            None => {
                // M/754's suffix_type_lost branch only borrows the format.
                // Keep the original cloud URL/size; never substitute catalog
                // media, quality, purchase flags or an ordinary authorization.
                let id = task.song_id.clone();
                let quality = task.options.quality;
                let fallback = self
                    .session
                    .run_for(&gate.token, |api, _| async move {
                        crate::catalog::CatalogApi::new(api)
                            .download_url(&json!(id), quality.as_str())
                            .await
                    })
                    .await?
                    .ok_or(Error::Response("cloud download format fallback missing"))?;
                gate.require()?;
                if fallback["code"] != 200 {
                    return Err(Error::Response("cloud download format fallback rejected"));
                }
                fallback["type"]
                    .as_str()
                    .filter(|s| {
                        !s.is_empty()
                            && s.len() <= 16
                            && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    })
                    .ok_or(Error::Response("cloud download format missing"))?
                    .to_owned()
            }
        };
        // No fabricated quality, fee, purchase or ordinary download flags.
        let mut resource = json!({"code":200,"id":task.song_id,"name":name,"size":size,"url":url,"type":suffix.to_ascii_lowercase(),"origin":"cloud"});
        if let Some(md5) = response.get("md5").filter(|v| !v.is_null()) {
            resource["md5"] = md5.clone();
        }
        task.song = Some(entry.song);
        Ok(resource)
    }
}
fn origin(entry: &CloudEntry) -> Result<DownloadOrigin> {
    identifier(&json!(entry.song_id))?;
    if entry.file_name.is_empty()
        || entry.file_size == 0
        || identifier(&entry.song["id"])? != entry.song_id
    {
        return Err(Error::InvalidInput("cloud original identity is incomplete"));
    }
    Ok(DownloadOrigin::Cloud {
        file_name: entry.file_name.clone(),
        file_size: entry.file_size,
    })
}
fn options(lyrics: bool) -> DownloadOptions {
    DownloadOptions {
        tags: false,
        cover: false,
        lyrics,
        container: OutputContainer::Audio,
        ..Default::default()
    }
}
