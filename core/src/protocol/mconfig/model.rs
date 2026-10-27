use crate::{Error, Result, protocol::http::ApiOrigin};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const APPLICATION_KEY: &str = "IuRPVVmc3WWul9fT";
pub(super) const OWNER: &str = "transport";
pub(super) const KIND: &str = "pc-mconfig-v1";
pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub origin: ApiOrigin,
    pub version: u64,
    pub bucket: String,
    pub generation: u64,
    pub source_url: Option<String>,
    pub data: Option<Value>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            origin: ApiOrigin::Pc,
            version: 0,
            bucket: String::new(),
            generation: 0,
            source_url: None,
            data: None,
        }
    }
}
impl Snapshot {
    pub(super) fn validate(&self) -> Result<()> {
        if self.origin != ApiOrigin::Pc
            || self.version > 9_007_199_254_740_991
            || self.bucket.len() > 4096
            || self.bucket.contains(['\r', '\n', '\0'])
            || self.data.as_ref().is_some_and(|v| !v.is_object())
            || (self.version == 0) != self.data.is_none()
            || self.data.is_some() != self.source_url.is_some()
        {
            return Err(Error::Response("invalid persisted PC configuration"));
        }
        if let Some(url) = &self.source_url {
            ConfigUrl::parse(url)?;
        }
        Ok(())
    }
    pub fn request_header(&self, app_version: &str) -> Result<String> {
        if app_version.is_empty()
            || app_version.len() > 128
            || app_version.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid MConfig application version"));
        }
        Ok(json!({APPLICATION_KEY:{"version":self.version,"appver":app_version}}).to_string())
    }
}

/// Only official configuration CDN objects; never music credentials or redirects.
#[derive(Clone)]
pub struct ConfigUrl(url::Url);
impl ConfigUrl {
    pub fn parse(raw: &str) -> Result<Self> {
        let url = url::Url::parse(raw).map_err(|_| Error::Response("invalid MConfig URL"))?;
        if raw.len() > 4096
            || url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || !url
                .host_str()
                .is_some_and(|h| h.ends_with(".music.126.net"))
            || !url.path().starts_with("/obj/")
            || !url.path().ends_with(".data")
        {
            return Err(Error::Response("unreviewed MConfig URL"));
        }
        Ok(Self(url))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub(super) fn header<'a>(metadata: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    metadata
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
pub(super) fn information(metadata: &BTreeMap<String, String>) -> Result<Value> {
    let Some(raw) = header(metadata, "mconfig-info") else {
        return Ok(Value::Null);
    };
    if raw.len() > 32 * 1024 {
        return Err(Error::Response("MConfig header exceeds limit"));
    }
    let v: Value =
        serde_json::from_str(raw).map_err(|_| Error::Response("invalid MConfig header"))?;
    Ok(v[APPLICATION_KEY].clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Update {
    Ignored,
    NonPc,
    Current,
    InProgress,
    Updated,
}
