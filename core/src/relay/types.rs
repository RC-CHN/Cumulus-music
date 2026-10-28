use crate::{Error, Result, playback::PlaybackMode, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(super) const MAX_RESOURCES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayResourceKind {
    Song,
    Voice,
}
impl RelayResourceKind {
    pub fn from_official(kind: &str) -> Result<Self> {
        match kind {
            "song" | "track" | "downloadTrack" | "localTrack" | "cloudTrack" => Ok(Self::Song),
            "voice" | "downloadVoice" | "localVoice" => Ok(Self::Voice),
            _ => Err(Error::Response("unknown relay resource type")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayResource {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: RelayResourceKind,
}
impl RelayResource {
    pub fn new(id: &Value, kind: RelayResourceKind) -> Result<Self> {
        let resource = Self {
            id: identifier(id)?,
            kind,
        };
        resource.validate()?;
        Ok(resource)
    }
    pub fn from_official(value: &Value) -> Result<Self> {
        Self::new(
            &value["id"],
            RelayResourceKind::from_official(
                value["type"]
                    .as_str()
                    .ok_or(Error::Response("missing relay resource type"))?,
            )?,
        )
    }
    pub(super) fn validate(&self) -> Result<()> {
        identifier(&json!(self.id))?;
        if self.id.len() > 128 {
            return Err(Error::InvalidInput("relay identifier too large"));
        }
        Ok(())
    }
}
pub(super) fn validate_resources(resources: &[RelayResource]) -> Result<()> {
    if resources.is_empty() || resources.len() > MAX_RESOURCES {
        return Err(Error::InvalidInput("invalid relay resources length"));
    }
    resources.iter().try_for_each(RelayResource::validate)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayMode {
    Order,
    Random,
    SingleLoop,
    ListLoop,
}
impl RelayMode {
    pub fn from_official(value: &str) -> Option<Self> {
        match value {
            "order" | "playOrder" => Some(Self::Order),
            "random" | "playRandom" => Some(Self::Random),
            "single_loop" | "playOneCycle" => Some(Self::SingleLoop),
            "list_loop" | "playCycle" => Some(Self::ListLoop),
            _ => None,
        }
    }
}
impl From<PlaybackMode> for RelayMode {
    fn from(mode: PlaybackMode) -> Self {
        match mode {
            PlaybackMode::Sequential => Self::Order,
            PlaybackMode::Random => Self::Random,
            PlaybackMode::SingleLoop => Self::SingleLoop,
            PlaybackMode::OrderLoop => Self::ListLoop,
        }
    }
}
impl From<RelayMode> for PlaybackMode {
    fn from(mode: RelayMode) -> Self {
        match mode {
            RelayMode::Order => Self::Sequential,
            RelayMode::Random => Self::Random,
            RelayMode::SingleLoop => Self::SingleLoop,
            RelayMode::ListLoop => Self::OrderLoop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelaySourceKind {
    Playlist,
    Album,
    Podcast,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelaySource {
    Collection {
        source_type: RelaySourceKind,
        id: String,
    },
    Resources {
        resources: Vec<RelayResource>,
    },
}
impl RelaySource {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Collection { id, .. } => {
                RelayResource::new(&json!(id), RelayResourceKind::Song).map(|_| ())
            }
            Self::Resources { resources } => validate_resources(resources),
        }
    }
    pub fn excluded_scene(scene: &str) -> bool {
        matches!(
            scene,
            "cloud" | "local" | "download" | "fm" | "zoneFm" | "aiMode"
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum RelayCommand {
    Insert {
        resources: Vec<RelayResource>,
        #[serde(rename = "preId", skip_serializing_if = "Option::is_none")]
        pre_id: Option<String>,
    },
    Delete {
        resources: Vec<RelayResource>,
    },
    Move {
        resources: Vec<RelayResource>,
        #[serde(rename = "preId", skip_serializing_if = "Option::is_none")]
        pre_id: Option<String>,
    },
    Clear,
}
impl RelayCommand {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Clear => Ok(()),
            Self::Delete { resources } => validate_resources(resources),
            Self::Insert { resources, pre_id } | Self::Move { resources, pre_id } => {
                validate_resources(resources)?;
                if matches!(self, Self::Move { .. }) && resources.len() != 1 {
                    return Err(Error::InvalidInput("move one relay resource"));
                }
                if let Some(id) = pre_id {
                    RelayResource::new(&json!(id), RelayResourceKind::Song)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConfig {
    pub enable: bool,
    pub radio_progress_interval: u64,
    pub snapshot_size: usize,
}
impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enable: false,
            radio_progress_interval: 20,
            snapshot_size: 100,
        }
    }
}
impl RelayConfig {
    pub fn from_data(value: &Value) -> Result<Self> {
        let mut config = Self::default();
        let object = value
            .as_object()
            .ok_or(Error::Response("missing relay configuration"))?;
        if let Some(enable) = object.get("enable") {
            config.enable = enable
                .as_bool()
                .ok_or(Error::Response("invalid relay enable flag"))?;
        }
        if let Some(interval) = object.get("radioProgressInterval") {
            config.radio_progress_interval = interval
                .as_u64()
                .ok_or(Error::Response("invalid relay progress interval"))?
                .max(3);
        }
        if let Some(size) = object.get("snapshotSize") {
            config.snapshot_size = usize::try_from(
                size.as_u64()
                    .ok_or(Error::Response("invalid relay snapshot size"))?,
            )
            .map_err(|_| Error::Response("relay snapshot size overflow"))?
            .max(100);
        }
        // Native resource limits reject hostile values instead of allocating an
        // unbounded queue or overflowing a timer. Not an invented server limit.
        if config.snapshot_size > MAX_RESOURCES || config.radio_progress_interval > 86_400 {
            return Err(Error::Response("relay configuration exceeds native bounds"));
        }
        Ok(config)
    }
}

/// Native-only alert: the official pull passes through every server field.
/// Hosts expose selected text and a local revision, never this raw object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelayAlert {
    raw: Value,
}
impl RelayAlert {
    pub fn from_official(raw: Value) -> Result<Self> {
        if !raw.is_object() || serde_json::to_vec(&raw)?.len() > 65_536 {
            return Err(Error::Response("invalid relay alert"));
        }
        validate_device(
            raw["deviceId"]
                .as_str()
                .ok_or(Error::Response("relay device missing"))?,
        )?;
        Ok(Self { raw })
    }
    pub fn device_id(&self) -> &str {
        self.raw["deviceId"].as_str().unwrap_or_default()
    }
    pub fn text(&self, key: &str) -> Option<&str> {
        if !matches!(
            key,
            "deviceName" | "resourceName" | "reconnectTitle" | "btnDesc"
        ) {
            return None;
        }
        self.raw[key].as_str().filter(|s| s.len() <= 1024)
    }
    pub(super) fn payload(&self) -> Result<Value> {
        Self::from_official(self.raw.clone())?;
        let mut value = self.raw.clone();
        value["targetDeviceId"] = json!(self.device_id());
        Ok(value)
    }
}
pub(super) fn validate_device(device: &str) -> Result<()> {
    if device.is_empty() || device.len() > 512 || device.chars().any(char::is_control) {
        return Err(Error::InvalidInput("invalid relay device identity"));
    }
    Ok(())
}
