use super::{control::Control, files, *};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    download::{
        naming::{PathRequest, plan_path},
        transfer::{CreateTransfer, MediaUrl, TransferResource},
    },
    media::tags::official_requires_encryption,
    membership::Quality,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::fs;
const ENDPOINT: &str = "/api/song/enhance/download/url/v1";
impl DownloadService {
    pub(super) async fn detail(&self, task: &mut DownloadTask, gate: &Control) -> Result<()> {
        if task.song.is_some() {
            return Ok(());
        }
        if task.origin.is_cloud() || task.origin.is_mv() || task.origin.is_voice() {
            return Err(Error::State("specialized download metadata missing"));
        }
        gate.require()?;
        let id = task.song_id.clone();
        let mut batch = self
            .session
            .run_for(&gate.token, |api, _| async move {
                CatalogApi::new(api).songs(&[json!(id)], 1).await
            })
            .await?;
        gate.require()?;
        let mut song = batch
            .songs
            .pop()
            .flatten()
            .ok_or(Error::Response("download song metadata missing"))?;
        if identifier(&song["id"])? != task.song_id {
            return Err(Error::Response("download metadata belongs to another song"));
        }
        if let Some(p) = batch.privileges.remove(&task.song_id) {
            song["privilege"] = p;
        }
        task.song = Some(song);
        self.save_current(&gate.token, task)
    }
    pub(super) async fn authorize(&self, task: &mut DownloadTask, gate: &Control) -> Result<()> {
        gate.require()?;
        task.status = DownloadStatus::Authorizing;
        self.save_current(&gate.token, task)?;
        let id = task.song_id.clone();
        let level = task.options.quality;
        let resource = if task.origin.is_cloud() {
            self.authorize_cloud(task, gate).await?
        } else if task.origin.is_mv() {
            self.authorize_mv(task, gate).await?
        } else if task.origin.is_voice() {
            self.authorize_voice(task, gate).await?
        } else {
            self.session
                .run_for(&gate.token, |api, _| async move {
                    CatalogApi::new(api)
                        .download_url(&json!(id), level.as_str())
                        .await
                })
                .await?
                .unwrap_or(Value::Null)
        };
        gate.require()?;
        task.authorization_count = task
            .authorization_count
            .checked_add(1)
            .ok_or(Error::State("authorization count overflow"))?;
        task.resource = Some(resource.clone());
        self.save_current(&gate.token, task)?;
        if resource["code"] != 200 || resource["url"].as_str().is_none_or(str::is_empty) {
            return Err(Error::service(ENDPOINT, resource["code"].clone(), resource));
        }
        if let Some(id) = resource.get("id").filter(|v| !v.is_null())
            && identifier(id)? != task.song_id
        {
            return Err(Error::Response(
                "download authorization belongs to another song",
            ));
        }
        if !resource["freeTrialInfo"].is_null() {
            return Err(Error::Response("trial media is not a full download"));
        }
        if let Some(level) = resource.get("level") {
            Quality::parse(
                level
                    .as_str()
                    .ok_or(Error::Response("invalid download quality"))?,
            )?;
        }
        let media = TransferResource {
            url: MediaUrl::parse(resource["url"].as_str().expect("checked"))?,
            size: resource
                .get("size")
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n > 0)
                        .ok_or(Error::Response("invalid authorized size"))
                })
                .transpose()?,
            md5: resource
                .get("md5")
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or(Error::Response("invalid authorized MD5"))
                })
                .transpose()?,
        };
        task.official_encryption =
            official_requires_encryption(word(&resource["flag"]), word(&resource["payed"]));
        task.encrypted = task.options.container == OutputContainer::Ncm
            || task.options.container == OutputContainer::Official && task.official_encryption;
        if task.encrypted {
            self.offline.refresh_membership(&gate.token).await?;
            gate.require()?;
        }
        self.detail(task, gate).await?;
        self.resolve_target(task, &resource)?;
        self.save_current(&gate.token, task)?;
        gate.require()?;
        if self
            .store
            .get(&self.owner, "media_transfers_v1", &task.id)?
            .is_some()
        {
            self.transfer.refresh_resource(&task.id, media)?;
        } else {
            self.transfer.create(CreateTransfer {
                resource: media,
                target: task.work.join("raw"),
                resource_id: Some(task.song_id.clone()),
                id: Some(task.id.clone()),
            })?;
        }
        task.status = DownloadStatus::Downloading;
        task.phase = DownloadPhase::Transfer;
        self.save_current(&gate.token, task)
    }
    fn resolve_target(&self, task: &mut DownloadTask, resource: &Value) -> Result<()> {
        files::work(task)?;
        let target = match &task.destination {
            DownloadDestination::Path { path } => {
                if task.encrypted {
                    path.with_extension("ncm")
                } else {
                    path.clone()
                }
            }
            DownloadDestination::Named {
                root,
                path_type,
                file_type,
            } => {
                let mut song = task
                    .song
                    .clone()
                    .ok_or(Error::State("missing naming metadata"))?;
                for (key, alias) in [("artists", "ar"), ("album", "al")] {
                    if song.get(key).is_none()
                        && let Some(value) = song.get(alias).cloned()
                    {
                        song[key] = value;
                    }
                }
                let suffix = if task.encrypted {
                    "ncm"
                } else {
                    resource["type"]
                        .as_str()
                        .ok_or(Error::Response("download resource lacks format"))?
                };
                let suffix = if suffix == "mp4" && resource["level"] == "vivid" {
                    "m4a"
                } else {
                    suffix
                };
                let relative = plan_path(&PathRequest {
                    path_type: *path_type,
                    file_type: *file_type,
                    track: if task.origin.is_mv() || task.origin.is_voice() {
                        None
                    } else {
                        Some(song.clone())
                    },
                    voice: if task.origin.is_voice() {
                        Some(song["voice"].clone())
                    } else {
                        None
                    },
                    mv: if task.origin.is_mv() {
                        Some(song)
                    } else {
                        None
                    },
                    offline_path_length: root.to_str().expect("validated").encode_utf16().count()
                        + 1,
                    suffix: suffix.into(),
                    yun_name: None,
                })?
                .relative;
                let mut parent = root.clone();
                for part in relative.parent().expect("relative parent").components() {
                    let std::path::Component::Normal(part) = part else {
                        return Err(Error::State("invalid generated path"));
                    };
                    parent.push(part);
                    match fs::symlink_metadata(&parent) {
                        Ok(m) if m.is_dir() => (),
                        Ok(_) => {
                            return Err(Error::State("generated download directory is redirected"));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            fs::create_dir(&parent)?
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                let parent = parent.canonicalize()?;
                if !parent.starts_with(root) {
                    return Err(Error::State("download path escapes root"));
                }
                parent.join(relative.file_name().expect("planned name"))
            }
        };
        files::absent(&target)?;
        task.target_parent = Some(files::identity(target.parent().expect("normalized"))?);
        task.target = Some(target);
        Ok(())
    }
}
fn word(value: &Value) -> u32 {
    value.as_u64().unwrap_or(0) as u32
}
