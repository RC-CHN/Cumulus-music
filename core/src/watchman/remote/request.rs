use super::super::{ReportKind, SdkBuild};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiHost {
    Primary,
    Secondary,
}
impl ApiHost {
    pub fn name(self) -> &'static str {
        match self {
            Self::Primary => "ac.dun.163.com",
            Self::Secondary => "ac.dun.163yun.com",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.strip_prefix("https://").unwrap_or(value);
        let value = value.strip_suffix('/').unwrap_or(value);
        match value {
            "ac.dun.163.com" => Ok(Self::Primary),
            "ac.dun.163yun.com" => Ok(Self::Secondary),
            _ => Err(Error::Response("unreviewed Watchman API host")),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}
#[derive(Clone)]
pub enum RequestKind {
    Configuration {
        product: String,
        cache_key: String,
        timestamp_ms: u64,
    },
    Report {
        build: SdkBuild,
        kind: ReportKind,
        packet: String,
    },
}
#[derive(Clone)]
pub struct Request {
    pub host: ApiHost,
    pub callback: String,
    pub kind: RequestKind,
}
impl Request {
    pub fn path(&self) -> &'static str {
        match self.kind {
            RequestKind::Configuration { .. } => "/v2/config/js",
            RequestKind::Report {
                kind: ReportKind::Device,
                ..
            } => "/v3/d",
            RequestKind::Report {
                kind: ReportKind::Behavior,
                ..
            } => "/v3/b",
        }
    }
    pub fn method(&self) -> HttpMethod {
        if matches!(self.kind, RequestKind::Configuration { .. }) {
            HttpMethod::Get
        } else {
            HttpMethod::Post
        }
    }
    pub fn timeout_ms(&self) -> u64 {
        if self.method() == HttpMethod::Get {
            6000
        } else {
            5000
        }
    }
    pub fn form(&self) -> Result<String> {
        if self.callback.is_empty()
            || self.callback.len() > 80
            || !self
                .callback
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(Error::InvalidInput("invalid Watchman JSONP callback"));
        }
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        match &self.kind {
            RequestKind::Configuration {
                product,
                cache_key,
                timestamp_ms,
            } => {
                if product.is_empty()
                    || product.len() > 64
                    || !product.bytes().all(|b| b.is_ascii_alphanumeric())
                    || cache_key.len() > 256
                    || cache_key.contains(['\r', '\n', '\0'])
                {
                    return Err(Error::InvalidInput(
                        "invalid Watchman configuration request",
                    ));
                }
                form.append_pair("pn", product)
                    .append_pair("cvk", cache_key)
                    .append_pair("cb", &self.callback)
                    .append_pair("t", &timestamp_ms.to_string());
            }
            RequestKind::Report { build, packet, .. } => {
                if packet.len() > (super::super::MAX_BYTES + 80).div_ceil(3) * 4
                    || packet.is_empty()
                    || !packet
                        .bytes()
                        .all(|b| build.alphabet().contains(&b) || b == build.padding())
                {
                    return Err(Error::InvalidInput("invalid Watchman report packet"));
                }
                form.append_pair("d", packet)
                    .append_pair("v", build.wire_version())
                    .append_pair("cb", &self.callback);
            }
        }
        Ok(form.finish())
    }
    pub fn url(&self) -> Result<String> {
        let base = format!("https://{}{}", self.host.name(), self.path());
        let form = self.form()?;
        Ok(if self.method() == HttpMethod::Get {
            format!("{base}?{form}")
        } else {
            base
        })
    }
    pub fn parse_response(&self, bytes: &[u8]) -> Result<Value> {
        if bytes.len() > super::super::MAX_BYTES {
            return Err(Error::Response("Watchman response exceeds limit"));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Response("Watchman response is not UTF-8"))?
            .trim();
        let text = text.strip_suffix(';').unwrap_or(text);
        let content = text
            .strip_prefix(&format!("{}(", self.callback))
            .and_then(|v| v.strip_suffix(')'))
            .ok_or(Error::Response("Watchman JSONP callback mismatch"))?;
        serde_json::from_str(content).map_err(|_| Error::Response("invalid Watchman JSONP data"))
    }
}
