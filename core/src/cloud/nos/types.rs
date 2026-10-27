use super::*;
use crate::cloud::source::Source;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NosNode(String);
impl NosNode {
    pub fn parse(input: &str) -> Result<Self> {
        let url = url::Url::parse(input).map_err(|_| Error::InvalidInput("invalid NOS node"))?;
        let host = url
            .host_str()
            .ok_or(Error::InvalidInput("missing NOS host"))?;
        let prefix = host
            .strip_suffix(".127.net")
            .and_then(|s| s.strip_prefix("nosup"));
        let valid = prefix.is_some_and(|s| {
            s.is_empty()
                || s.strip_prefix('-').is_some_and(|s| {
                    !s.is_empty()
                        && s.split('-').all(|p| {
                            !p.is_empty()
                                && p.bytes()
                                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                        })
                })
        });
        if !valid
            || !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some_and(|p| p != 80 && p != 443)
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidInput("unrecognized NOS node"));
        }
        Ok(Self(format!("https://{host}")))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Private native persistence contains the scoped token. Never send this type
/// to the frontend; NosView is its public projection. Intentionally no Debug.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NosAllocation {
    pub(super) bucket: String,
    pub(super) key: String,
    pub(super) token: String,
    pub(super) resource: String,
}
impl NosAllocation {
    pub fn parse(value: &Value) -> Result<Self> {
        if value["channel"].as_u64() != Some(1) {
            return Err(Error::InvalidInput("allocation is not NOS channel 1"));
        }
        let get = |name: &str| -> Result<String> {
            let text = match &value[name] {
                Value::String(s) => s.clone(),
                Value::Number(n) if n.is_u64() || n.is_i64() => n.to_string(),
                _ => return Err(Error::Response("invalid NOS allocation")),
            };
            valid_text(&text, 16384)?;
            Ok(text)
        };
        let bucket = get("bucket")?;
        valid_bucket(&bucket)?;
        let key = get("objectKey")?;
        valid_text(&key, 4096)?;
        if matches!(key.as_str(), "." | "..") {
            return Err(Error::Response("invalid NOS object key"));
        }
        if value
            .get("key")
            .is_some_and(|v| !v.is_null() && v != "" && v != &Value::String(key.clone()))
        {
            return Err(Error::Response("conflicting NOS object keys"));
        }
        let allocation = Self {
            bucket,
            key,
            token: get("token")?,
            resource: get("resourceId")?,
        };
        allocation.validate()?;
        Ok(allocation)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        valid_bucket(&self.bucket)?;
        valid_text(&self.key, 4096)?;
        if matches!(self.key.as_str(), "." | "..") {
            return Err(Error::InvalidInput("invalid NOS object key"));
        }
        valid_token(&self.token)?;
        valid_text(&self.resource, 16384)
    }
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    pub fn object_key(&self) -> &str {
        &self.key
    }
    pub fn resource_id(&self) -> &str {
        &self.resource
    }
    pub(super) fn same_object(&self, other: &Self) -> bool {
        self.bucket == other.bucket && self.key == other.key && self.resource == other.resource
    }
}
pub(super) fn valid_text(s: &str, max: usize) -> Result<()> {
    if s.is_empty() || s.chars().count() > max || s.contains(['\r', '\n', '\0']) {
        Err(Error::InvalidInput("invalid NOS allocation text"))
    } else {
        Ok(())
    }
}
pub(super) fn valid_token(s: &str) -> Result<()> {
    valid_text(s, 16384)?;
    reqwest::header::HeaderValue::from_str(s)
        .map_err(|_| Error::InvalidInput("invalid scoped NOS token"))?;
    Ok(())
}
pub(super) fn valid_bucket(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 255
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        Err(Error::InvalidInput("invalid NOS bucket"))
    } else {
        Ok(())
    }
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err(Error::InvalidInput("invalid upload identifier"))
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NosStatus {
    Queued,
    Uploading,
    Paused,
    Error,
    Ambiguous,
    Uploaded,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NosRenewal {
    Pending,
    Ready,
    Confirmed,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NosControl {
    Pause,
    Cancel,
}
#[derive(Clone, Debug, Serialize)]
pub struct NosView {
    pub id: String,
    pub status: NosStatus,
    pub source: PathBuf,
    pub bytes: u64,
    pub total: u64,
    pub pending: bool,
    pub renewal: Option<NosRenewal>,
    pub requested: Option<NosControl>,
    pub error: Option<String>,
}
#[derive(Clone, Default)]
pub struct NosRunOptions {
    pub chunk_budget: Option<usize>,
    pub stop: Arc<AtomicBool>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Pending {
    pub offset: u64,
    pub end: u64,
    pub complete: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Renewal {
    pub status: NosRenewal,
    pub attempts: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Job {
    pub id: String,
    pub source: Source,
    pub allocation: NosAllocation,
    pub node: NosNode,
    pub content_type: String,
    pub chunk_size: usize,
    pub status: NosStatus,
    pub offset: u64,
    pub context: String,
    pub pending: Option<Pending>,
    pub renewal: Option<Renewal>,
    pub error: Option<String>,
}
impl Job {
    pub fn view(&self, requested: Option<NosControl>) -> NosView {
        NosView {
            id: self.id.clone(),
            status: self.status,
            source: self.source.path.clone(),
            bytes: self.offset,
            total: self.source.bytes,
            pending: self.pending.is_some(),
            renewal: self.renewal.as_ref().map(|r| r.status),
            requested,
            error: self.error.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        self.source.validate()?;
        self.allocation.validate()?;
        NosNode::parse(self.node.as_str())?;
        if self.offset > self.source.bytes
            || !(1..=4 * 1024 * 1024).contains(&self.chunk_size)
            || self.context.len() > 16384
            || self.context.contains(['\r', '\n', '\0'])
        {
            return Err(Error::State("invalid persisted NOS progress"));
        }
        if let Some(p) = &self.pending
            && (p.offset != self.offset
                || p.end <= p.offset
                || p.end > self.source.bytes
                || p.complete != (p.end == self.source.bytes))
        {
            return Err(Error::State("invalid pending NOS chunk"));
        }
        valid_mime(&self.content_type)
    }
}
pub(super) fn valid_mime(s: &str) -> Result<()> {
    let valid = s.split_once('/').is_some_and(|(a, b)| {
        [a, b].iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".+-".contains(&b))
        })
    });
    if !valid || s.len() > 255 {
        Err(Error::InvalidInput("invalid upload MIME type"))
    } else {
        Ok(())
    }
}
