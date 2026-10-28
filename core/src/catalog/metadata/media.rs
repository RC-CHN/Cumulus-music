use super::ArtistCredit;
use crate::{Error, Result, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoKind {
    Mv,
    Video,
    Unknown,
}
pub struct VideoCardMetadata {
    pub id: String,
    pub kind: VideoKind,
    pub title: String,
    pub description: String,
    pub artists: Vec<ArtistCredit>,
    pub cover_url: String,
    pub duration_ms: u64,
    pub play_count: u64,
    pub mv_id: Option<String>,
}
fn text(a: &Value, b: &Value) -> String {
    a.as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| b.as_str())
        .unwrap_or_default()
        .into()
}
fn count(a: &Value, b: &Value) -> u64 {
    a.as_u64()
        .filter(|n| *n != 0)
        .or_else(|| b.as_u64())
        .unwrap_or(0)
}
impl VideoCardMetadata {
    /// M/21 V card fields. MV details/playback still require MvService reads.
    pub fn from_official(value: &Value) -> Result<Self> {
        let id = value["id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| value["id"].as_u64().map(|n| n.to_string()))
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 256
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            })
            .ok_or(Error::Response("invalid catalog video ID"))?;
        let resource = value
            .get("resource")
            .filter(|v| v.is_object())
            .ok_or(Error::Response("missing catalog video resource"))?;
        let base = &resource["mlogBaseData"];
        let extra = &resource["mlogExtVO"];
        let data = &resource["data"];
        let kind = if value["isMv"] == true {
            VideoKind::Mv
        } else {
            match base["type"]
                .as_u64()
                .or_else(|| base["type"].as_str()?.parse().ok())
            {
                Some(3) => VideoKind::Mv,
                Some(2) => VideoKind::Video,
                _ => VideoKind::Unknown,
            }
        };
        let artists = extra
            .get("artists")
            .filter(|v| !v.is_null())
            .unwrap_or(&data["artists"]);
        let mut artists: Vec<ArtistCredit> = artists
            .as_array()
            .into_iter()
            .flatten()
            .map(ArtistCredit::from_official)
            .collect();
        if artists.is_empty() {
            let name = text(&extra["artistName"], &data["artistName"]);
            if !name.is_empty() {
                artists.push(ArtistCredit { id: None, name });
            }
        }
        Ok(Self {
            mv_id: (kind == VideoKind::Mv)
                .then(|| identifier(&json!(id)).ok())
                .flatten(),
            id,
            kind,
            title: text(&base["text"], &data["name"]),
            description: text(&base["desc"], &data["desc"]),
            artists,
            cover_url: text(&base["coverUrl"], &data["cover"]),
            duration_ms: count(&base["duration"], &data["duration"]),
            play_count: count(&extra["playCount"], &data["playCount"]),
        })
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ConcertDate {
    Timestamp(i64),
    Text(String),
}
pub struct ConcertMetadata {
    pub id: String,
    pub title: String,
    pub cover_url: String,
    pub start: Option<ConcertDate>,
    pub end: Option<ConcertDate>,
    pub address: String,
    pub price: String,
    /// Native only: never include the server navigation URL in a UI DTO.
    pub url: String,
}
fn date(value: &Value) -> Option<ConcertDate> {
    value.as_i64().map(ConcertDate::Timestamp).or_else(|| {
        value
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .map(|s| ConcertDate::Text(s.into()))
    })
}
impl ConcertMetadata {
    /// Recovered ShowCard/index.tsx, used by artist ShowTab in M/3088.
    pub fn from_official(value: &Value) -> Result<Self> {
        Ok(Self {
            id: identifier(&value["id"])?,
            title: super::text(&value["title"]),
            cover_url: super::text(&value["cover"]),
            start: date(&value["time"][0]),
            end: date(&value["time"][1]),
            address: super::text(&value["simpleAddress"]["address"]),
            price: match &value["price"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            },
            url: super::text(&value["url"]),
        })
    }
}
