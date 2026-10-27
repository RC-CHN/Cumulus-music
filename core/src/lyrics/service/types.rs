use super::super::ParsedLyrics;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct LyricDemand {
    pub desktop: bool,
    pub detail_page: bool,
    pub mini_mode: bool,
    pub pet: bool,
    /// Overrides the demand check, not the same-track deduplication.
    pub force: bool,
}
impl LyricDemand {
    pub(super) fn any(&self) -> bool {
        self.desktop || self.detail_page || self.mini_mode || self.pet || self.force
    }
}

#[derive(Clone, Debug)]
pub enum LyricTarget {
    Song(String),
    Cloud(String),
    Local(PathBuf),
}
impl LyricTarget {
    pub fn resource_key(&self) -> crate::Result<String> {
        super::request::key(self)
    }
}

#[derive(Clone, Debug)]
pub struct LyricRequest {
    pub target: LyricTarget,
    pub demand: LyricDemand,
    pub downloaded_audio: Option<PathBuf>,
    pub track_mv: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricPreferences {
    pub translation_enable: bool,
    pub roma_enable: bool,
}
impl Default for LyricPreferences {
    fn default() -> Self {
        Self {
            translation_enable: true,
            roma_enable: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricSnapshot {
    pub resource_key: Option<String>,
    pub is_loading: bool,
    pub is_lyric_fetch_failed: bool,
    pub network_failed: bool,
    pub from_cache: bool,
    pub from_sidecar: bool,
    pub preferences: LyricPreferences,
    pub lyric: ParsedLyrics,
}
