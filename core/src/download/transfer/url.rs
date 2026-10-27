use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// A service-provided CDN URL. Serialized only into private native storage;
/// never put this type into a frontend DTO or a diagnostic log.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct MediaUrl(pub(super) String);
impl MediaUrl {
    pub fn parse(input: &str) -> Result<Self> {
        if input.len() > 16384
            || input.contains('\\')
            || input.chars().any(|c| c.is_control())
            || input.trim() != input
        {
            return Err(Error::InvalidInput("invalid media URL text"));
        }
        let mut url =
            url::Url::parse(input).map_err(|_| Error::InvalidInput("invalid media URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port().is_some_and(|p| p != 80 && p != 443)
        {
            return Err(Error::InvalidInput("invalid media URL authority"));
        }
        let host = url
            .host_str()
            .ok_or(Error::InvalidInput("media URL requires a host"))?;
        if ![
            "music.126.net",
            "music.163.com",
            "netease.com",
            "vod.126.net",
        ]
        .iter()
        .any(|base| host.ends_with(&format!(".{base}")))
        {
            return Err(Error::InvalidInput("unrecognized media CDN"));
        }
        url.set_scheme("https")
            .map_err(|_| Error::InvalidInput("invalid media URL scheme"))?;
        url.set_port(None)
            .map_err(|_| Error::InvalidInput("invalid media URL port"))?;
        Ok(Self(url.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for MediaUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaUrl(<redacted>)")
    }
}
impl<'de> Deserialize<'de> for MediaUrl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
