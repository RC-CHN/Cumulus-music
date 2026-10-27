//! Recovered M/23 K/Y and M/35 purchase-mask decisions for mapped local music.
//! This is a client policy check, not a server authorization or new DRM scheme.
use super::{MembershipState, Quality};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub enum OfflineSource {
    Imported,
    Downloaded { complete: bool },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineDenial {
    MissingPrivilege,
    DownloadIncomplete,
    Album,
    Vip,
    Svip,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineAccess {
    pub can_play: bool,
    pub reason: Option<OfflineDenial>,
}
fn allow() -> OfflineAccess {
    OfflineAccess {
        can_play: true,
        reason: None,
    }
}
fn deny(reason: OfflineDenial) -> OfflineAccess {
    OfflineAccess {
        can_play: false,
        reason: Some(reason),
    }
}

fn integer(value: Option<&Value>, default: i64) -> Result<i64> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(number)) => number
            .as_i64()
            .ok_or(Error::Response("non-integer offline privilege")),
        Some(Value::String(value)) => value
            .parse()
            .map_err(|_| Error::Response("invalid offline privilege text")),
        _ => Err(Error::Response("invalid offline privilege type")),
    }
}
pub fn offline_access(
    privilege: Option<&Value>,
    quality: Quality,
    membership: &MembershipState,
    source: OfflineSource,
    encrypted: bool,
) -> Result<OfflineAccess> {
    if matches!(source, OfflineSource::Downloaded { complete: false }) {
        return Ok(deny(OfflineDenial::DownloadIncomplete));
    }
    if !encrypted {
        return Ok(allow());
    }
    let Some(privilege) = privilege.filter(|p| p.is_object()) else {
        return Ok(deny(OfflineDenial::MissingPrivilege));
    };
    // Official Number(null)==0; missing fee remains undefined/NaN and cannot
    // independently establish a free resource. Malformed types fail closed.
    if privilege.get("fee").is_some() && integer(privilege.get("fee"), 0)? == 0 {
        return Ok(allow());
    }
    let flag = integer(privilege.get("flag"), 0)?;
    let (allowed, reason) = if flag & (1 << 13) != 0 {
        (membership.is_album_vip, OfflineDenial::Album)
    } else if quality.requires_plus_offline() {
        (membership.is_plus_vip, OfflineDenial::Svip)
    } else {
        let downloaded = matches!(source, OfflineSource::Downloaded { complete: true });
        let paid = if downloaded {
            integer(privilege.get("payed"), 0)? & 6 != 0
        } else {
            false
        };
        (
            membership.is_vinyl_vip || downloaded && (paid || membership.is_music_package),
            OfflineDenial::Vip,
        )
    };
    Ok(if allowed { allow() } else { deny(reason) })
}
