use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TYPES_PATH: &str = "/api/feedback/correct/lyric/type";
pub const SUBMIT_PATH: &str = "/api/feedback/correct/song";

/// M3085 TextArea and submit handler count UTF-16 units without trimming.
pub fn validate_content(content: &str) -> Result<()> {
    if !(5..=50).contains(&content.encode_utf16().count()) {
        return Err(Error::InvalidInput(
            "lyric report requires 5 to 50 UTF-16 units",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub song_id: String,
    pub title: String,
    /// M370 selects LRC first, including when YRC words are also displayed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyric_version: Option<Value>,
}
impl Target {
    pub(super) fn validate(&self) -> Result<()> {
        identifier(&json!(self.song_id))?;
        if self.title.len() > 4096
            || self.lyric_version.as_ref().is_some_and(|v| {
                !v.is_null() && !v.is_number() && !v.is_string()
                    || v.as_str().is_some_and(|v| v.len() > 128)
            })
        {
            return Err(Error::InvalidInput("invalid lyric report target"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorrectionType {
    pub key: String,
    pub label: String,
    pub value: i64,
}
/// M109 C excludes literal keys 1/2/3, then Object.entries numeric-key order.
pub fn parse_types(value: &Value) -> Result<Vec<CorrectionType>> {
    let values = value["data"]
        .as_object()
        .ok_or(Error::Response("invalid lyric report types"))?;
    let mut result = Vec::new();
    for (key, label) in values {
        if ["1", "2", "3"].contains(&key.as_str()) {
            continue;
        }
        let number = key
            .parse::<i64>()
            .ok()
            .filter(|v| *v >= 0)
            .ok_or(Error::Response("invalid lyric report type key"))?;
        let label = label
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 4096)
            .ok_or(Error::Response("invalid lyric report type label"))?;
        result.push(CorrectionType {
            key: key.clone(),
            label: label.into(),
            value: number,
        });
    }
    let index = |key: &str| {
        key.parse::<u32>()
            .ok()
            .filter(|v| *v < u32::MAX && v.to_string() == key)
    };
    result.sort_by(|a, b| match (index(&a.key), index(&b.key)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Draft,
    Rejected,
    Sending,
    Uncertain,
    Sent,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    NotSent,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub revision: String,
    pub target: Target,
    pub options: Vec<CorrectionType>,
    pub selected: Option<String>,
    pub content: String,
    pub phase: Phase,
    pub issue: Option<Issue>,
}
impl Record {
    pub(super) fn validate(&self) -> Result<()> {
        uuid::Uuid::parse_str(&self.id).map_err(|_| Error::State("invalid lyric report ID"))?;
        uuid::Uuid::parse_str(&self.revision)
            .map_err(|_| Error::State("invalid lyric report revision"))?;
        self.target.validate()?;
        let mut keys = std::collections::BTreeSet::new();
        for option in &self.options {
            if !keys.insert(&option.key)
                || ["1", "2", "3"].contains(&option.key.as_str())
                || option.key.parse::<i64>().ok() != Some(option.value)
                || option.label.is_empty()
                || option.label.len() > 4096
            {
                return Err(Error::State("invalid saved lyric report types"));
            }
        }
        if !self.content.is_empty() {
            validate_content(&self.content)?;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|v| !self.options.iter().any(|o| &o.key == v))
        {
            return Err(Error::State("invalid saved lyric report choice"));
        }
        if matches!(self.phase, Phase::Sending | Phase::Uncertain | Phase::Sent) {
            validate_content(&self.content)?;
            if self.selected.is_none() {
                return Err(Error::State("lyric report lacks submitted choice"));
            }
        }
        Ok(())
    }
}
