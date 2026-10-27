//! Native effect records. Raw records/material URLs must not cross frontend IPC.
use crate::{
    Error, Result,
    download::transfer::{MediaUrl, TransferResource},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct EffectId(u64);
impl EffectId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 || value > 9_007_199_254_740_991 {
            return Err(Error::InvalidInput(
                "effect ID must be an exact positive JS integer",
            ));
        }
        Ok(Self(value))
    }
    pub fn parse(value: &Value) -> Result<Self> {
        Self::new(
            identifier(value)?
                .parse()
                .map_err(|_| Error::InvalidInput("effect ID out of range"))?,
        )
    }
    pub fn value(self) -> u64 {
        self.0
    }
    pub fn interests_type(self) -> u32 {
        if matches!(self.0, 200..=202) {
            116
        } else {
            127
        }
    }
}
impl TryFrom<u64> for EffectId {
    type Error = Error;
    fn try_from(value: u64) -> Result<Self> {
        Self::new(value)
    }
}
impl From<EffectId> for u64 {
    fn from(value: EffectId) -> u64 {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogueKind {
    HighlyRecommended,
    Recommended,
    More,
}
impl CatalogueKind {
    pub fn expose_type(self) -> u32 {
        match self {
            Self::HighlyRecommended => 5,
            Self::Recommended => 6,
            Self::More => 7,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sound {
    pub id: EffectId,
    pub name: String,
    pub subtitle: String,
    /// Product classification from the catalogue, NOT the detail request type.
    pub catalogue_sound_type: Option<u32>,
    raw: Value,
}
impl Sound {
    pub fn from_official(value: &Value) -> Result<Self> {
        if !value.is_object() {
            return Err(Error::Response("sound record must be an object"));
        }
        Ok(Self {
            id: EffectId::parse(&value["id"])?,
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            subtitle: value["subTitle"].as_str().unwrap_or_default().to_owned(),
            catalogue_sound_type: value["soundType"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok()),
            raw: value.clone(),
        })
    }
    /// M/342 chooses index zero. Do not silently switch to another material.
    pub fn parameter_material(&self) -> Result<Option<MaterialDescriptor>> {
        let Some(materials) = self.raw.get("soundMaterialList").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let materials = materials
            .as_array()
            .ok_or(Error::Response("invalid sound material list"))?;
        materials
            .first()
            .map(MaterialDescriptor::from_official)
            .transpose()
    }
    pub fn catalogue_authorization(&self) -> Option<super::rights::Authorization> {
        self.raw
            .get("vipAuthRespItem")
            .and_then(super::rights::Authorization::from_official)
    }
    pub fn native_record(&self) -> &Value {
        &self.raw
    }
}

#[derive(Clone, Serialize)]
pub struct MaterialDescriptor {
    resource: TransferResource,
}
impl<'de> Deserialize<'de> for MaterialDescriptor {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            resource: TransferResource,
        }
        let stored = Stored::deserialize(deserializer)?.resource;
        Self::from_fields(
            &serde_json::json!(stored.url.as_str()),
            &serde_json::json!(stored.md5),
            &serde_json::json!(stored.size),
        )
        .map_err(serde::de::Error::custom)
    }
}
impl MaterialDescriptor {
    pub fn from_official(value: &Value) -> Result<Self> {
        if value["type"].as_u64() != Some(1) {
            return Err(Error::Response("unsupported sound material type"));
        }
        Self::from_fields(&value["link"], &value["md5"], &value["size"])
    }
    pub fn from_fields(url: &Value, md5: &Value, size: &Value) -> Result<Self> {
        let size = size
            .as_u64()
            .filter(|v| *v > 0 && *v <= 64 * 1024 * 1024)
            .ok_or(Error::Response("invalid sound material size"))?;
        let md5 = md5
            .as_str()
            .filter(|v| v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or(Error::Response("invalid sound material MD5"))?
            .to_ascii_lowercase();
        let url = MediaUrl::parse(
            url.as_str()
                .ok_or(Error::Response("missing sound material URL"))?,
        )?;
        Ok(Self {
            resource: TransferResource {
                url,
                size: Some(size),
                md5: Some(md5),
            },
        })
    }
    pub fn size(&self) -> u64 {
        self.resource.size.expect("validated material size")
    }
    pub fn md5(&self) -> &str {
        self.resource
            .md5
            .as_deref()
            .expect("validated material digest")
    }
    pub fn resource(&self) -> TransferResource {
        self.resource.clone()
    }
}
