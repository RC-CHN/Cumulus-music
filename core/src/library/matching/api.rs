use crate::{
    Error, Result,
    library::LibraryItem,
    protocol::{
        api::{Api, identifier},
        wire::serialization::{encode_component, js_json, js_truthy},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchQuery {
    pub title: String,
    pub album: String,
    pub artist: String,
    pub duration: f64,
    pub persist_id: String,
}
impl MatchQuery {
    pub fn from_item(item: &LibraryItem) -> Result<Self> {
        let duration = item.metadata.duration;
        if !duration.is_finite() || duration < 0.0 || item.id.is_empty() || item.id.contains('?') {
            return Err(Error::InvalidInput(
                "invalid local matching identity or duration",
            ));
        }
        let artist = item.metadata.artists.first().cloned().unwrap_or_default();
        let joined = format!(
            "{}-{}-{}",
            item.metadata.title,
            artist,
            ryu_js::Buffer::new().format(duration)
        );
        // JS encodeURIComponent, including its !'()* exceptions. URL form
        // encoding is different (space/+ and punctuation) and must not be used.
        let escaped = encode_component(&joined);
        Ok(Self {
            title: item.metadata.title.clone(),
            album: item.metadata.album.clone(),
            artist,
            duration,
            persist_id: format!("{}?{}", item.id, escaped),
        })
    }
}

pub fn normalize_matches(
    result: &Value,
    queries: &[MatchQuery],
) -> Result<BTreeMap<String, Value>> {
    let ids = result["ids"]
        .as_array()
        .ok_or(Error::Response("missing match identities"))?;
    let songs = result["songs"]
        .as_array()
        .ok_or(Error::Response("missing matched songs"))?;
    if ids.len() != songs.len() || ids.len() > queries.len() {
        return Err(Error::Response("mismatched match response lengths"));
    }
    let expected = queries
        .iter()
        .map(|q| q.persist_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut out = BTreeMap::new();
    for (id, song) in ids.iter().zip(songs) {
        if song.is_null() {
            continue;
        }
        let id = id
            .as_str()
            .filter(|id| expected.contains(id))
            .ok_or(Error::Response("foreign local matching identity"))?;
        if !seen.insert(id) {
            return Err(Error::Response("duplicate local matching identity"));
        }
        if !song.is_object() {
            return Err(Error::Response("invalid matched song"));
        }
        if js_truthy(&song["djProgramId"]) {
            continue;
        }
        identifier(&song["id"]).map_err(|_| Error::Response("invalid matched song identity"))?;
        let mut song = song.clone();
        if !js_truthy(&song["duration"]) {
            song["duration"] = if js_truthy(&song["playTime"]) {
                song["playTime"].clone()
            } else {
                json!(0)
            };
        }
        out.insert(
            id.split_once('?')
                .ok_or(Error::Response("invalid local matching identity"))?
                .0
                .to_owned(),
            song,
        );
    }
    Ok(out)
}
pub struct MatchingApi {
    api: Arc<dyn Api>,
}
impl MatchingApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn match_batch(&self, queries: &[MatchQuery]) -> Result<BTreeMap<String, Value>> {
        if queries.is_empty() || queries.len() > 50 {
            return Err(Error::InvalidInput(
                "local matching batch must contain 1 to 50 songs",
            ));
        }
        let mut identities = BTreeSet::new();
        for q in queries {
            let Some((local, _)) = q.persist_id.split_once('?') else {
                return Err(Error::InvalidInput("invalid persistId"));
            };
            if local.is_empty()
                || !identities.insert(local)
                || !q.duration.is_finite()
                || q.duration < 0.0
            {
                return Err(Error::InvalidInput("invalid matching query"));
            }
        }
        let response = self
            .api
            .call(
                "/api/search/match/new",
                json!({"songs":js_json(&serde_json::to_value(queries)?)?}),
                false,
                &[200],
            )
            .await?;
        normalize_matches(&response["result"], queries)
    }
}
