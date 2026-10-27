use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SharedMode {
    #[default]
    OrderLoop,
    Random,
    SingleLoop,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub user_id: String,
    pub version: u64,
}
impl Version {
    pub fn parse(v: &Value) -> Result<Vec<Self>> {
        let rows = v
            .as_array()
            .ok_or(Error::Response("missing queue version vector"))?
            .iter()
            .map(|v| {
                Ok(Self {
                    user_id: super::model::id(&v["userId"])?,
                    version: v["version"]
                        .as_u64()
                        .ok_or(Error::Response("invalid queue version"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        validate_versions(&rows)?;
        Ok(rows)
    }
    pub fn increment(rows: &[Self], owner: &str) -> Result<Vec<Self>> {
        identifier(&json!(owner))?;
        validate_versions(rows)?;
        let mut rows = rows.to_vec();
        if let Some(v) = rows.iter_mut().find(|v| v.user_id == owner) {
            v.version = v
                .version
                .checked_add(1)
                .ok_or(Error::State("queue version exhausted"))?;
        } else {
            rows.push(Self {
                user_id: owner.into(),
                version: 1,
            });
        }
        Ok(rows)
    }
}
fn validate_versions(rows: &[Version]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for row in rows {
        identifier(&json!(row.user_id))?;
        if !ids.insert(&row.user_id) {
            return Err(Error::Response("duplicate queue version owner"));
        }
    }
    Ok(())
}
/// M/137 e: unequal lengths short-circuit to false (verified by executing it).
pub fn versions_equal(a: &[Version], b: &[Version]) -> bool {
    validate_versions(a).is_ok()
        && validate_versions(b).is_ok()
        && a.len() == b.len()
        && a.iter().all(|v| b.contains(v))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ListCommandKind {
    Add,
    Delete,
    Replace,
    PlaymodeChange,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCommand {
    pub command_type: ListCommandKind,
    pub version: Vec<Version>,
    pub play_mode: SharedMode,
    pub anchor_song_id: String,
    pub anchor_position: i64,
    pub random_list: Vec<String>,
    pub display_list: Vec<String>,
}
impl ListCommand {
    pub fn validate(&self) -> Result<()> {
        validate_versions(&self.version)?;
        if self.version.is_empty() {
            return Err(Error::InvalidInput("empty outgoing version vector"));
        }
        let display = super::model::ids(&json!(self.display_list))?;
        let random = super::model::ids(&json!(self.random_list))?;
        if display.iter().collect::<BTreeSet<_>>() != random.iter().collect() {
            return Err(Error::InvalidInput("outgoing shared permutation differs"));
        }
        if self.command_type == ListCommandKind::Add {
            if !self.anchor_song_id.is_empty() {
                identifier(&json!(self.anchor_song_id))?;
            }
            if !(-1..1000).contains(&self.anchor_position) {
                return Err(Error::InvalidInput("invalid shared queue anchor"));
            }
        } else if !self.anchor_song_id.is_empty() || self.anchor_position != -1 {
            return Err(Error::InvalidInput("only ADD has a queue anchor"));
        }
        Ok(())
    }
    pub fn wire(&self) -> Result<Value> {
        self.validate()?;
        let mut v = serde_json::to_value(self)?;
        // M/3079 uses Number(uid). Serialize integer IDs exactly without f64.
        v["version"] = Value::Array(self.version.iter().map(|entry| {
            Ok(json!({"userId":entry.user_id.parse::<u64>().map_err(|_|Error::InvalidInput("queue owner exceeds numeric wire ID"))?,"version":entry.version}))
        }).collect::<Result<Vec<_>>>()?);
        Ok(v)
    }
}
/// M/137: crop BEFORE de-duplication; the current track controls the window.
/// Callers resolve local files to online IDs and filter unsupported resources.
pub fn shared_window(ids: &[String], current: Option<&str>) -> Result<Vec<String>> {
    for id in ids {
        identifier(&json!(id))?;
    }
    let window = if ids.len() <= 1000 {
        ids
    } else {
        let position = current.and_then(|c| ids.iter().position(|id| id == c));
        let start = match position {
            None | Some(0..=999) => 0,
            Some(p) if ids.len() - p < 500 => ids.len() - 1000,
            Some(p) => p - 500,
        };
        &ids[start..start + 1000]
    };
    let mut seen = BTreeSet::new();
    Ok(window
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect())
}
