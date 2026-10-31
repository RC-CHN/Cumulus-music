//! Official M/86.e -> M/15.n intelligence recommendations. This is the
//! transport/formatting contract; entering/refilling/exiting AI playback belongs
//! to its coordinator, not to a replacement random-queue implementation.
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

const PATH: &str = "/api/playmode/intelligence/list";
// Match the existing complete-playlist bound; count is the original liked-list
// size, not a page size to truncate or silently replace with a fixed limit.
pub const MAX_LIKED_TRACKS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntelligenceStart {
    #[serde(rename = "fromPlayAll")]
    PlayAll,
    #[serde(rename = "fromPlayOne")]
    PlayOne,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntelligenceRequest {
    pub playlist_id: String,
    pub song_id: String,
    pub start_music_id: String,
    pub start: IntelligenceStart,
    pub count: usize,
}
impl IntelligenceRequest {
    pub fn payload(&self) -> Result<Value> {
        if self.count == 0 || self.count > MAX_LIKED_TRACKS {
            return Err(Error::InvalidInput("invalid intelligence liked-list count"));
        }
        Ok(json!({
            "playlistId":identifier(&json!(self.playlist_id))?,
            "songId":identifier(&json!(self.song_id))?,
            "type":self.start,
            "startMusicId":identifier(&json!(self.start_music_id))?,
            "count":self.count,
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceSongSource {
    SongInfo,
    Liked,
    Unresolved,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntelligenceEntry {
    pub id: String,
    /// Raw catalog metadata plus recovered algorithm decoration. No database
    /// side effects or frontend-normalized song objects cross this boundary.
    pub song: Option<Value>,
    pub source: IntelligenceSongSource,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntelligenceRecommendation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended: Option<Value>,
    #[serde(rename = "alg", skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntelligenceBatch {
    /// Preserve row order and duplicate rows. A missing songInfo and missing
    /// liked fallback stays explicit instead of inventing playable metadata.
    pub entries: Vec<IntelligenceEntry>,
    /// M/15 overwrites repeated IDs in this map while retaining all track rows.
    pub recommendations: BTreeMap<String, IntelligenceRecommendation>,
}

pub struct IntelligenceApi {
    api: Arc<dyn Api>,
}
impl IntelligenceApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn recommend(
        &self,
        request: &IntelligenceRequest,
        liked: &[Value],
    ) -> Result<IntelligenceBatch> {
        let payload = request.payload()?;
        if liked.len() > MAX_LIKED_TRACKS {
            return Err(Error::InvalidInput("too many intelligence liked tracks"));
        }
        let mut liked = liked
            .iter()
            .map(|song| {
                Ok((
                    identifier(&song["id"])?,
                    song.as_object()
                        .cloned()
                        .ok_or(Error::InvalidInput("invalid intelligence liked song"))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        // M/15 cacheStrategy g.enable=false. Local starPlaylist and uid are not
        // request fields: M/86 forwards only the five fields above.
        let response = self.api.call(PATH, payload, false, &[200]).await?;
        let rows = response["data"]
            .as_array()
            .ok_or(Error::Response("missing intelligence recommendations"))?;
        if rows.len() > MAX_LIKED_TRACKS {
            return Err(Error::Response("too many intelligence recommendations"));
        }
        let mut entries = Vec::with_capacity(rows.len());
        let mut recommendations = BTreeMap::new();
        for row in rows {
            let id = identifier(&row["id"])
                .map_err(|_| Error::Response("invalid intelligence recommendation id"))?;
            let algorithm = row.get("alg");
            let (song, source) = if truthy(&row["songInfo"]) {
                let mut song = row["songInfo"]
                    .as_object()
                    .cloned()
                    .ok_or(Error::Response("invalid intelligence songInfo"))?;
                identifier(&song.get("id").cloned().unwrap_or(Value::Null))
                    .map_err(|_| Error::Response("invalid intelligence songInfo id"))?;
                match algorithm {
                    Some(value) => {
                        song.insert("alg".into(), value.clone());
                    }
                    None => {
                        song.remove("alg");
                    }
                }
                // M/21.x's algorithm field uses t.alg || ''. Other metadata is
                // intentionally raw; the existing catalog/player maps own it.
                song.insert(
                    "algorithm".into(),
                    algorithm
                        .filter(|v| truthy(v))
                        .cloned()
                        .unwrap_or_else(|| json!("")),
                );
                (Some(Value::Object(song)), IntelligenceSongSource::SongInfo)
            } else if let Some(song) = liked.get_mut(&id) {
                match algorithm {
                    Some(value) => {
                        song.insert("algorithm".into(), value.clone());
                    }
                    None => {
                        song.remove("algorithm");
                    }
                }
                (None, IntelligenceSongSource::Liked)
            } else {
                (None, IntelligenceSongSource::Unresolved)
            };
            entries.push(IntelligenceEntry {
                id: id.clone(),
                song,
                source,
            });
            recommendations.insert(
                id.clone(),
                IntelligenceRecommendation {
                    id,
                    recommended: row.get("recommended").cloned(),
                    algorithm: algorithm.cloned(),
                },
            );
        }
        // Original fallback rows alias the same liked object. All references
        // observe the final fallback algorithm for that ID after Promise.all.
        // Reproduce that result with private clones, without mutating callers.
        for entry in &mut entries {
            if entry.source == IntelligenceSongSource::Liked {
                entry.song = Some(Value::Object(liked[&entry.id].clone()));
            }
        }
        Ok(IntelligenceBatch {
            entries,
            recommendations,
        })
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
