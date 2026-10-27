use crate::{
    Error, Result,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(super) fn first<'a>(v: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter()
        .map(|k| &v[*k])
        .find(|v| js_truthy(v))
        .unwrap_or(&Value::Null)
}
pub(super) fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").into()
}
pub(super) fn count(v: &Value) -> Result<u64> {
    if v.is_null() {
        return Ok(0);
    }
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .ok_or(Error::Response("invalid podcast count"))
}
pub(super) fn rows<'a>(v: &'a Value, field: &str) -> Result<&'a Vec<Value>> {
    v[field]
        .as_array()
        .ok_or(Error::Response("podcast list missing"))
}

/// M/21 Z, retaining native source fields needed for fees and tags.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Radio {
    pub id: String,
    pub name: String,
    pub cover_url: String,
    pub description: String,
    pub program_count: u64,
    pub subscribed: Option<bool>,
    pub raw: Value,
}
impl Radio {
    pub fn from_official(v: &Value) -> Result<Self> {
        if !v.is_object() {
            return Err(Error::Response("podcast metadata missing"));
        }
        Ok(Self {
            id: identifier(first(v, &["id", "resourceId", "voiceListId", "productId"]))?,
            name: text(first(v, &["name", "voiceListName"])),
            cover_url: text(first(v, &["coverImgUrl", "picUrl", "coverUrl"])),
            description: text(first(v, &["description", "desc"])),
            program_count: count(first(v, &["voiceCount", "programCount"]))?,
            subscribed: v["subed"].as_bool().or_else(|| v["subscribed"].as_bool()),
            raw: v.clone(),
        })
    }
    pub fn normalized(&self) -> Value {
        let mut value = self.raw.clone();
        value["id"] = json!(self.id);
        value["name"] = json!(self.name);
        value["coverImgUrl"] = json!(self.cover_url);
        value["programCount"] = json!(self.program_count);
        value
    }
}

/// M/21 W. Collection/history cards may omit mainSong; only detail/permission
/// resolution can make such a card playable. No program ID is used as song ID.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub name: String,
    pub song_id: Option<String>,
    pub radio: Option<Radio>,
    pub duration_ms: u64,
    pub cover_url: String,
    pub liked: Option<bool>,
    pub raw: Value,
}
impl Episode {
    pub fn from_official(v: &Value) -> Result<Self> {
        if !v.is_object() {
            return Err(Error::Response("program metadata missing"));
        }
        let song = first(v, &["mainTrackId"]);
        let song = if song.is_null() {
            first(&v["mainSong"], &["id"])
        } else {
            song
        };
        let song = if song.is_null() {
            first(v, &["trackId"])
        } else {
            song
        };
        let mut duration = first(v, &["duration"]);
        if duration.is_null() {
            duration = first(&v["mainSong"], &["duration", "dt"]);
        }
        Ok(Self {
            id: identifier(first(v, &["id", "resourceId", "voiceId", "productId"]))?,
            name: text(first(v, &["name", "voiceName"])),
            song_id: if song.is_null() {
                None
            } else {
                Some(identifier(song)?)
            },
            radio: if v["radio"].is_null() {
                None
            } else {
                Some(Radio::from_official(&v["radio"])?)
            },
            duration_ms: count(duration)?,
            cover_url: text(&v["coverUrl"]),
            liked: v["liked"].as_bool(),
            raw: v.clone(),
        })
    }
    pub fn media_id(&self) -> Result<&str> {
        self.song_id
            .as_deref()
            .ok_or(Error::Response("program main song missing"))
    }
    pub fn normalized(&self) -> Value {
        let mut value = self.raw.clone();
        value["id"] = json!(self.id);
        value["programId"] = json!(self.id);
        value["name"] = json!(self.name);
        value["duration"] = json!(self.duration_ms);
        value["mainTrackId"] = json!(self.song_id);
        value["album"] = json!({});
        value["track"] = self.raw["mainSong"].clone();
        if let Some(radio) = &self.radio {
            value["audio"] = radio.normalized();
        }
        if value["dj"].is_null() && !value["creator"].is_null() {
            value["dj"] = value["creator"].clone();
        }
        if let Some(id) = value["dj"].get("userId").cloned() {
            value["dj"]["id"] = id;
        }
        value
    }
    pub fn playback_resource(&self) -> Result<Value> {
        Ok(json!({"id":self.media_id()?,"voice_id":self.id,
            "metadata":{"duration":self.duration_ms as f64/1000.0},"voice":self.normalized()}))
    }
    pub fn download_metadata(&self) -> Result<Value> {
        let mut track = self.raw["mainSong"].clone();
        if !track.is_object() {
            track = json!({});
        }
        track["id"] = json!(self.media_id()?);
        track["voice_id"] = json!(self.id);
        track["name"] = json!(self.name);
        track["duration"] = json!(self.duration_ms);
        let voice = self.normalized();
        track["artists"] = json!([{"id":voice["dj"]["id"],"name":voice["dj"]["nickname"]}]);
        track["album"] = json!({"name":self.radio.as_ref().map(|v|v.name.as_str()).unwrap_or(""),"picUrl":self.cover_url});
        track["voice"] = voice;
        Ok(track)
    }
}
