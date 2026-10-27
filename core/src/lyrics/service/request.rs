use super::super::response::{normalize_cloud_response, normalize_song_response};
use super::types::LyricTarget;
use crate::{
    Error, Result,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn key(target: &LyricTarget) -> Result<String> {
    match target {
        LyricTarget::Song(id) => Ok(format!("song:{}", identifier(&json!(id))?)),
        LyricTarget::Cloud(id) => Ok(format!("cloud:{}", identifier(&json!(id))?)),
        LyricTarget::Local(path) => {
            if !path.is_absolute() {
                return Err(Error::InvalidInput("absolute local audio path required"));
            }
            // JSON serializes native paths only when lossless UTF-8 is possible.
            let bytes = serde_json::to_vec(path)?;
            Ok(format!("local:{}", hex::encode(Sha256::digest(bytes))))
        }
    }
}

pub(super) async fn fetch(
    api: &dyn Api,
    uid: &str,
    target: &LyricTarget,
    cached: Option<&Value>,
) -> Result<Value> {
    let version = |key: &str| {
        cached
            .and_then(|c| c[key].get("version"))
            .filter(|v| js_truthy(v))
            .cloned()
            .unwrap_or(json!(-1))
    };
    match target {
        LyricTarget::Song(id) => {
            let raw=api.call("/api/song/lyric/v1",json!({"id":id,"lv":version("lrc"),"tv":version("tlyric"),"rv":version("romalrc"),"yv":version("yrc")}),false,&[200]).await?;
            normalize_song_response(&raw)
        }
        LyricTarget::Cloud(id) => {
            let raw = api
                .call(
                    "/api/cloud/lyric/get",
                    json!({"songId":id,"userId":uid,"lv":version("lrc"),"kv":-1}),
                    false,
                    &[200],
                )
                .await?;
            normalize_cloud_response(&raw)
        }
        LyricTarget::Local(_) => Err(Error::State("local lyrics do not use network")),
    }
}
