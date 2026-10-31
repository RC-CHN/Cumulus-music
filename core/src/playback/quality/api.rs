use crate::{
    Error, Result,
    membership::Quality,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityFormat {
    Stereo,
    Surround51,
    Aac,
}
impl QualityFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stereo => "ste",
            Self::Surround51 => "c51",
            Self::Aac => "aac",
        }
    }
}
/// PC resource types from M/100 y. Linux uses the same already verified PC API
/// domain; these are service contract IDs, not a claim about output hardware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityRight {
    Vivid,
    JyMaster,
    Sky,
    JyEffect,
}
impl QualityRight {
    pub const ALL: [Self; 4] = [Self::Vivid, Self::JyMaster, Self::Sky, Self::JyEffect];
    pub fn resource_type(self) -> u32 {
        match self {
            Self::Vivid => 41,
            Self::JyMaster => 31,
            Self::Sky => 32,
            Self::JyEffect => 33,
        }
    }
    pub fn quality(self) -> Quality {
        match self {
            Self::Vivid => Quality::Vivid,
            Self::JyMaster => Quality::JyMaster,
            Self::Sky => Quality::Sky,
            Self::JyEffect => Quality::JyEffect,
        }
    }
    pub fn for_quality(quality: Quality) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.quality() == quality)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialPhase {
    Eligible,
    Active,
    Granted,
    Unavailable,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualityUseRight {
    pub can_use: bool,
    pub user_source: Option<u32>,
    pub trial_status: Option<u32>,
    pub remaining_ms: Option<u64>,
}
impl QualityUseRight {
    pub fn phase(&self) -> TrialPhase {
        if !self.can_use {
            TrialPhase::Unavailable
        } else {
            match (self.user_source, self.trial_status) {
                (Some(0), Some(1)) => TrialPhase::Eligible,
                (Some(0), Some(2)) => TrialPhase::Active,
                (Some(source), _) if source > 0 => TrialPhase::Granted,
                _ => TrialPhase::Unknown,
            }
        }
    }
    fn from_official(value: &Value) -> Result<Self> {
        let can_use = value["canUse"]
            .as_bool()
            .ok_or(Error::Response("invalid quality use right"))?;
        let number = |key: &str| -> Result<Option<u64>> {
            match value.get(key).filter(|v| !v.is_null()) {
                None => Ok(None),
                Some(v) => v
                    .as_u64()
                    .filter(|v| *v <= 9_007_199_254_740_991)
                    .map(Some)
                    .ok_or(Error::Response("invalid quality right number")),
            }
        };
        let narrow = |key: &str| -> Result<Option<u32>> {
            number(key)?
                .map(|v| {
                    u32::try_from(v).map_err(|_| Error::Response("invalid quality right state"))
                })
                .transpose()
        };
        Ok(Self {
            can_use,
            user_source: narrow("userSrc")?,
            trial_status: narrow("trialStatus")?,
            remaining_ms: number("leftTime")?
                .filter(|v| *v > 0)
                .map(|v| v.min(i32::MAX as u64)),
        })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QualitySizes(BTreeMap<String, u64>);
impl QualitySizes {
    pub fn bytes(&self, quality: Quality) -> Option<u64> {
        self.0.get(quality.as_str()).copied()
    }
}
#[derive(Clone)]
pub struct QualityApi {
    api: Arc<dyn Api>,
}
impl QualityApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn sizes(&self, song: &Value, format: QualityFormat) -> Result<QualitySizes> {
        let song = identifier(song)?;
        let response = self
            .api
            .call(
                "/api/song/music/detail/get",
                json!({"songId":song,"immerseType":format.as_str()}),
                false,
                &[200],
            )
            .await?;
        let data = response["data"]
            .as_object()
            .ok_or(Error::Response("quality sizes missing"))?;
        if let Some(id) = data.get("id").filter(|v| !v.is_null())
            && identifier(id)? != song
        {
            return Err(Error::Response("quality size identity mismatch"));
        }
        let mut sizes = BTreeMap::new();
        for (field, quality) in [
            ("l", Quality::Standard),
            ("m", Quality::Higher),
            ("h", Quality::ExHigh),
            ("sq", Quality::Lossless),
            ("hr", Quality::HiRes),
            ("db", Quality::Dolby),
            ("je", Quality::JyEffect),
            ("jm", Quality::JyMaster),
            ("sk", Quality::Sky),
            ("vi", Quality::Vivid),
        ] {
            if let Some(row) = data.get(field).filter(|v| !v.is_null()) {
                if !row.is_object() {
                    return Err(Error::Response("invalid quality size row"));
                }
                if let Some(size) = row.get("size").filter(|v| !v.is_null()) {
                    let size = size
                        .as_u64()
                        .ok_or(Error::Response("invalid quality byte size"))?;
                    if size > 0 {
                        sizes.insert(quality.as_str().into(), size);
                    }
                }
            }
        }
        Ok(QualitySizes(sizes))
    }
    pub async fn use_right(&self, right: QualityRight) -> Result<Option<QualityUseRight>> {
        let response = self
            .api
            .call(
                "/api/vipauth/app/auth/query",
                json!({"resourceIdList":0,"resourceTypeList":right.resource_type()}),
                false,
                &[200],
            )
            .await?;
        let rows = response["data"]
            .as_array()
            .filter(|v| v.len() <= 1)
            .ok_or(Error::Response("ambiguous quality use rights"))?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        if row
            .get("resourceId")
            .filter(|v| !v.is_null())
            .is_some_and(|v| v.as_u64() != Some(0))
            || row
                .get("resourceType")
                .filter(|v| !v.is_null())
                .is_some_and(|v| v.as_u64() != Some(u64::from(right.resource_type())))
        {
            return Err(Error::Response("quality use right identity mismatch"));
        }
        QualityUseRight::from_official(row).map(Some)
    }
    /// Native endpoint primitive. Interactive hosts must add explicit consent
    /// and durable reconciliation before exposing it. A read never calls it.
    pub async fn activate(&self, right: QualityRight) -> Result<()> {
        let response = self
            .api
            .call(
                "/api/vipauth/app/auth/set",
                json!({"resourceId":0,"resourceType":right.resource_type()}),
                false,
                &[200],
            )
            .await?;
        if response["data"].as_bool() != Some(true) {
            return Err(Error::Response(
                "quality trial has no positive acknowledgement",
            ));
        }
        Ok(())
    }
}
