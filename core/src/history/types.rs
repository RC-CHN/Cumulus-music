use crate::{
    Error, Result,
    catalog::metadata::AlbumMetadata,
    podcast::{Episode, Radio},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    Song,
    Album,
    Playlist,
    Voice,
    Podcast,
    Audiobook,
}
impl HistoryKind {
    pub const ALL: [Self; 6] = [
        Self::Song,
        Self::Album,
        Self::Playlist,
        Self::Voice,
        Self::Podcast,
        Self::Audiobook,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Voice => "voice",
            Self::Podcast => "podcast",
            Self::Audiobook => "audiobook",
        }
    }
    pub fn wire(self) -> &'static str {
        match self {
            Self::Song => "SONG",
            Self::Album => "ALBUM",
            Self::Playlist => "PLAYLIST",
            Self::Voice => "VOICE",
            Self::Podcast => "DJ_RADIO",
            Self::Audiobook => "VOICE_BOOK",
        }
    }
    pub fn local_limit(self) -> usize {
        if self == Self::Song { 300 } else { 100 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordOrigin {
    Local,
    Remote,
}

/// Native metadata, not an IPC DTO. No URL from this structure is safe for UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub played_at: u64,
    pub resource: Value,
    pub device: Option<String>,
    pub origin: RecordOrigin,
    pub play_progress: Option<f64>,
    pub listened: Option<bool>,
}
impl HistoryRecord {
    pub fn local(kind: HistoryKind, resource: Value, played_at: u64) -> Result<Self> {
        let (id, resource) = normalize(kind, resource, true)?;
        Ok(Self {
            id,
            resource,
            played_at,
            device: None,
            origin: RecordOrigin::Local,
            play_progress: None,
            listened: None,
        })
    }
    pub(super) fn validate(&self, kind: HistoryKind) -> Result<()> {
        let (id, _) = normalize(
            kind,
            self.resource.clone(),
            self.origin == RecordOrigin::Local,
        )?;
        if id != self.id
            || self
                .play_progress
                .is_some_and(|p| !p.is_finite() || p < 0.0)
        {
            return Err(Error::Response("invalid history identity or progress"));
        }
        Ok(())
    }
}
pub(super) fn normalize(
    kind: HistoryKind,
    mut resource: Value,
    local: bool,
) -> Result<(String, Value)> {
    if !resource.is_object() {
        return Err(Error::Response("missing history resource"));
    }
    if local
        && kind == HistoryKind::Song
        && let Some(id) = resource["local_id"].as_str()
    {
        if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
            return Err(Error::InvalidInput("invalid local history ID"));
        }
        return Ok((format!("local:{id}"), resource));
    }
    let id = match kind {
        HistoryKind::Album => AlbumMetadata::from_official(&resource)?.id,
        HistoryKind::Voice => {
            let voice = Episode::from_official(&resource)?;
            resource = voice.normalized();
            voice.id
        }
        HistoryKind::Podcast | HistoryKind::Audiobook => {
            let radio = Radio::from_official(&resource)?;
            resource = radio.normalized();
            radio.id
        }
        HistoryKind::Song | HistoryKind::Playlist => {
            let value = ["id", "resourceId", "playlistId"]
                .into_iter()
                .map(|k| &resource[k])
                .find(|v| !v.is_null())
                .unwrap_or(&Value::Null);
            identifier(value)?
        }
    };
    resource["id"] = json!(id);
    Ok((id, resource))
}
