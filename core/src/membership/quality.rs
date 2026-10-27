use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "64aac")]
    Aac64,
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "higher")]
    Higher,
    #[serde(rename = "exhigh")]
    ExHigh,
    #[serde(rename = "lossless")]
    Lossless,
    #[serde(rename = "hires")]
    HiRes,
    #[serde(rename = "dolby")]
    Dolby,
    #[serde(rename = "jyeffect")]
    JyEffect,
    #[serde(rename = "jymaster")]
    JyMaster,
    #[serde(rename = "sky")]
    Sky,
    #[serde(rename = "vivid")]
    Vivid,
}
impl Quality {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "64aac" => Ok(Self::Aac64),
            "standard" => Ok(Self::Standard),
            "higher" => Ok(Self::Higher),
            "exhigh" => Ok(Self::ExHigh),
            "lossless" => Ok(Self::Lossless),
            "hires" => Ok(Self::HiRes),
            "dolby" => Ok(Self::Dolby),
            "jyeffect" => Ok(Self::JyEffect),
            "jymaster" => Ok(Self::JyMaster),
            "sky" => Ok(Self::Sky),
            "vivid" => Ok(Self::Vivid),
            _ => Err(Error::InvalidInput("unsupported quality level")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aac64 => "64aac",
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::ExHigh => "exhigh",
            Self::Lossless => "lossless",
            Self::HiRes => "hires",
            Self::Dolby => "dolby",
            Self::JyEffect => "jyeffect",
            Self::JyMaster => "jymaster",
            Self::Sky => "sky",
            Self::Vivid => "vivid",
        }
    }
    pub fn bitrate(self) -> u32 {
        match self {
            Self::Aac64 => 64000,
            Self::Standard => 128000,
            Self::Higher => 192000,
            Self::ExHigh => 320000,
            Self::Lossless => 999000,
            Self::HiRes => 1999000,
            Self::Dolby => 2999000,
            Self::JyEffect => 3999000,
            Self::JyMaster => 4999000,
            Self::Sky => 5999000,
            Self::Vivid => 6999000,
        }
    }
    pub fn from_bitrate(value: u32) -> Result<Self> {
        [
            Self::Aac64,
            Self::Standard,
            Self::Higher,
            Self::ExHigh,
            Self::Lossless,
            Self::HiRes,
            Self::Dolby,
            Self::JyEffect,
            Self::JyMaster,
            Self::Sky,
            Self::Vivid,
        ]
        .into_iter()
        .find(|level| level.bitrate() == value)
        .ok_or(Error::InvalidInput("unsupported quality bitrate"))
    }
    pub fn requires_plus_offline(self) -> bool {
        matches!(self, Self::Dolby | Self::Sky | Self::JyMaster)
    }
}
