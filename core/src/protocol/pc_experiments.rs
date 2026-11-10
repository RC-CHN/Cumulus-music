//! The recovered PC M147 assignment plan and group projection for 3.1.42.205594.
//! This module does not perform requests, exposure reports or native setting writes.

use crate::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::OnceLock,
};

pub const ASSIGNMENT_PATH: &str = "/api/rtrs/abt/front/expinfo/list";
pub const ANONYMOUS_PATH: &str = "/api/register/anonimous";

/// Native-only anonymous registration state. An anonymous UID never grants
/// the authenticated AccountSession namespace or enters a renderer DTO.
#[derive(Clone, PartialEq, Eq)]
pub enum PcAnonymousState {
    Unsupported,
    Unregistered,
    Uncertain,
    Registered { uid: String, revision: String },
}
const SKIP: &str = "PH-PC-skipSongs2";
const SEARCH_AUDIO_TOPLIST: &str = "pc-search-audio-toplist";
const EMOTION: &str = "PH-PC-qingxv";
const TAG: &str = "PH-PC-tagT";
const TAG_DEPENDENTS: [&str; 5] = [
    "PH-PC-VIP-songPlay",
    "PH-PC-recommendMore",
    "PH-PC-Home-refresh",
    "PH-PC-Home-Ranklist",
    "PH-PC-optimizeFM",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    // In JS object insertion order, including the platform-closed defaults.
    web: Vec<(String, bool, String)>,
    native: Vec<String>,
}

fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("pc_experiments_3_1_42.json"))
            .expect("the checked-in PC experiment manifest must be valid")
    })
}

/// M/147 appends the exact native vector after the open web keys. This is the
/// logical request only; the original signed transport may override its verb.
pub fn assignment_arguments() -> Value {
    let manifest = manifest();
    let names = manifest
        .web
        .iter()
        .filter(|(_, open, _)| *open)
        .map(|(name, _, _)| name.as_str())
        .chain(manifest.native.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(",");
    json!({ "expNames": names })
}

/// A group-only, bounded projection. No experiment log, clientConfig or
/// opaque assignment field is needed for the recovered queue decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcExperimentGroups {
    web: BTreeMap<&'static str, Option<String>>,
}

impl PcExperimentGroups {
    pub fn from_startup_cache(cache: &Value) -> Self {
        let web = manifest()
            .web
            .iter()
            .map(|(key, open, default)| {
                let group = if *open && key != EMOTION {
                    cache
                        .get(key)
                        .map(read_group)
                        .unwrap_or_else(|| Some(default.clone()))
                } else {
                    Some(default.clone())
                };
                (key.as_str(), group)
            })
            .collect();
        Self { web }
    }

    /// A successfully decoded response replaces open keys from their first
    /// matching row; a missing key returns to its original default.
    pub fn refreshed(response: &Value) -> Result<Self> {
        let rows = response["data"]
            .as_array()
            .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))
            .ok_or(Error::Protocol("invalid PC experiment assignment rows"))?;
        let mut first = HashMap::new();
        for row in rows {
            if let Some(name) = row["expName"].as_str() {
                first.entry(name).or_insert(row);
            }
        }
        let mut result = Self::from_startup_cache(&Value::Null);
        for (key, open, _) in &manifest().web {
            if *open && let Some(row) = first.get(key.as_str()) {
                result.web.insert(key.as_str(), read_group(row));
            }
        }
        if !matches!(result.group(TAG), Some("c" | "t1")) {
            for key in TAG_DEPENDENTS {
                result.web.insert(key, Some(String::new()));
            }
        }
        Ok(result)
    }

    /// The archived catch branch resets only the emotion experiment.
    pub fn after_refresh_failure(mut self) -> Self {
        let emotion = manifest()
            .web
            .iter()
            .find(|(key, _, _)| key == EMOTION)
            .map(|(_, _, group)| group.clone());
        self.web.insert(EMOTION, emotion);
        self
    }

    pub fn group(&self, name: &str) -> Option<&str> {
        self.web.get(name).and_then(Option::as_deref)
    }

    pub fn search_audio_toplist_assigned(&self) -> bool {
        self.group(SEARCH_AUDIO_TOPLIST) == Some("t1")
    }

    pub fn skip_songs_assigned(&self) -> bool {
        self.group(SKIP) == Some("t1")
    }

    /// M/34.i: on PC the new effect page also requires the new playback bar.
    pub fn effect_catalogue_new_assigned(&self) -> bool {
        self.group("PH-PC-yinxiaogaiban2") == Some("t1")
            && (self.group("PH-PC-minibarNew") == Some("t1")
                || (self.group("PH-PC-minibarNew") == Some("c")
                    && self.group("PH-PC-minibarNewLike") == Some("t1")))
    }
}

fn read_group(value: &Value) -> Option<String> {
    value["expGroupName"]
        .as_str()
        .filter(|group| group.len() <= 128 && !group.chars().any(char::is_control))
        .map(str::to_owned)
}
