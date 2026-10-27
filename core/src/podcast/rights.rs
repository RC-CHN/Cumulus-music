use super::{Episode, PodcastApi, model::rows};
use crate::{
    Error, Result,
    catalog::{CatalogApi, PlayOptions},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayPermission {
    pub voice_id: String,
    pub song_id: Option<String>,
    pub can_play: bool,
    pub reason_type: Value,
    pub fee_configs: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadPermission {
    pub allowed: Vec<String>,
    pub forbidden: Vec<String>,
    pub results: Vec<Value>,
    pub message: String,
}
impl PodcastApi {
    pub async fn play_permission(&self, id: &Value) -> Result<PlayPermission> {
        let id = identifier(id)?;
        let value = self
            .api
            .call(
                "/api/dj/play/url/check",
                json!({"voiceId":id}),
                false,
                &[200],
            )
            .await?;
        let data = &value["data"];
        if identifier(&data["voiceId"])? != id {
            return Err(Error::Response("program permission identity mismatch"));
        }
        let can_play = data["canPlay"]
            .as_bool()
            .ok_or(Error::Response("program permission missing"))?;
        let song_id = if can_play {
            Some(identifier(&data["songId"])?)
        } else {
            None
        };
        Ok(PlayPermission {
            voice_id: id,
            song_id,
            can_play,
            reason_type: data["cannotPlayReasonType"].clone(),
            fee_configs: data["feeConfigs"].clone(),
        })
    }
    pub async fn download_permission(&self, ids: &[Value]) -> Result<DownloadPermission> {
        if ids.is_empty() || ids.len() > 500 {
            return Err(Error::InvalidInput("invalid program permission batch"));
        }
        let wanted = ids
            .iter()
            .map(identifier)
            .collect::<Result<BTreeSet<_>>>()?;
        if wanted.len() != ids.len() {
            return Err(Error::InvalidInput("duplicate program permission ID"));
        }
        // M/404: the misleading Str suffix still takes an array, not JSON text.
        let value = self
            .api
            .call(
                "/api/dj/voices/download/check",
                json!({"voiceIdsStr":ids}),
                false,
                &[200],
            )
            .await?;
        let data = &value["data"];
        let allowed = rows(data, "allowDownloadVoiceIds")?
            .iter()
            .map(identifier)
            .collect::<Result<Vec<_>>>()?;
        let forbidden = rows(data, "forbidDownloadVoiceIds")?
            .iter()
            .map(identifier)
            .collect::<Result<Vec<_>>>()?;
        let mut covered = BTreeSet::new();
        for id in allowed.iter().chain(&forbidden) {
            if !wanted.contains(id) || !covered.insert(id.clone()) {
                return Err(Error::Response(
                    "invalid program download permission identities",
                ));
            }
        }
        if covered != wanted {
            return Err(Error::Response("incomplete program download permissions"));
        }
        let results = rows(data, "downloadCheckResultList")?.clone();
        let mut result_ids = BTreeSet::new();
        for result in &results {
            let id = identifier(&result["voiceId"])?;
            if !wanted.contains(&id)
                || !result_ids.insert(id.clone())
                || result["canDownload"].as_bool() != Some(allowed.contains(&id))
            {
                return Err(Error::Response("conflicting program download permission"));
            }
        }
        Ok(DownloadPermission {
            allowed,
            forbidden,
            results,
            message: data["forbidDownloadMsg"].as_str().unwrap_or("").into(),
        })
    }
    pub async fn play_url(&self, episode: &Episode, options: PlayOptions<'_>) -> Result<Value> {
        let permission = self.play_permission(&json!(episode.id)).await?;
        if !permission.can_play {
            return Err(Error::State("program playback entitlement unavailable"));
        }
        if permission.song_id.as_deref() != Some(episode.media_id()?) {
            return Err(Error::Response("program main song permission mismatch"));
        }
        CatalogApi::new(self.api.clone())
            .play_url(&json!(episode.media_id()?), options)
            .await?
            .ok_or(Error::Response("program media missing"))
    }
    pub async fn download_url(&self, episode: &Episode, level: &str) -> Result<Value> {
        let permission = self.download_permission(&[json!(episode.id)]).await?;
        if !permission.allowed.contains(&episode.id) {
            return Err(Error::State("program download entitlement unavailable"));
        }
        let mut resource = CatalogApi::new(self.api.clone())
            .download_url(&json!(episode.media_id()?), level)
            .await?
            .ok_or(Error::Response("program download media missing"))?;
        // M/754: even a successful URL response cannot turn a new-user
        // limited-free listening entitlement into an offline download.
        if resource["code"] == 200 && episode.raw["icon"]["type"] == "NEWUSERLIMITFREE" {
            return Err(Error::service(
                "/api/song/enhance/download/url/v1",
                json!(-144),
                resource,
            ));
        }
        // M/754 assigns br first and overrides it only for a known level.
        // Voice URLs currently return level=null with br=128000. Our offline
        // ledger requires a quality enum, so derive it from exact known bitrate
        // codes and preserve the original server level as a native fact.
        if resource["code"] == 200 && resource["level"].is_null() {
            let br = resource["br"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or(Error::Response("program actual bitrate missing"))?;
            let quality = crate::membership::Quality::from_bitrate(br)?;
            resource["official_level"] = resource["level"].clone();
            resource["level"] = json!(quality.as_str());
            resource["level_origin"] = json!("official_bitrate");
        }
        Ok(resource)
    }
}
