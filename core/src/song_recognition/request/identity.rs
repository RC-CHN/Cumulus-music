//! Deterministic recognition identity fields, CloudMusic PC 3.1.42.205594 x64.
//! All host observations are explicit. Nothing here collects machine identity
//! or authorizes any request.

use crate::{
    Error, Result,
    identity::{client_sign_with_separator, valid_persistent_id},
};
use sha1::{Digest, Sha1};

pub(super) const MAX_NATIVE_IDENTITY_BYTES: usize = 512;
const MAX_ADAPTER_ROWS: usize = 256;
const NO_TOKEN_USER_SID: &str = "neteasecloudmusic";

/// No Debug or serialization: fields are only used in the sealed request
/// fixture. This is distinct from the ordinary ClientProfile clientSign.
pub struct NativeClientSign(Box<str>);

impl NativeClientSign {
    /// Mac, disk, board, persistent ID in recovered Windows UTF-16 units.
    /// Empty hardware fields are explicit native observations. This bounded
    /// subset excludes malformed UTF-16 and control characters; it does not
    /// reproduce Windows collector conversions or persistent file recovery.
    pub fn from_wide(parts: [&[u16]; 4]) -> Result<Self> {
        let mut decoded = [String::new(), String::new(), String::new(), String::new()];
        for (part, output) in parts.into_iter().zip(&mut decoded) {
            if part.len() > MAX_NATIVE_IDENTITY_BYTES {
                return Err(Error::InvalidInput(
                    "recognition identity field exceeds its bound",
                ));
            }
            *output = String::from_utf16(part)
                .map_err(|_| Error::InvalidInput("invalid recognition identity UTF-16"))?;
            if output.chars().any(char::is_control) {
                return Err(Error::InvalidInput("invalid recognition identity text"));
            }
        }
        if !valid_persistent_id(&decoded[3]) {
            return Err(Error::InvalidInput(
                "invalid recognition persistent identity",
            ));
        }
        // 32DA52 literal UTF-16 @@@, passed through CEE370 to CEDF80.
        let sign = client_sign_with_separator(decoded.each_ref().map(String::as_str), 15, "@@@")?;
        if sign.len() > MAX_NATIVE_IDENTITY_BYTES {
            return Err(Error::InvalidInput(
                "recognition clientSign exceeds its bound",
            ));
        }
        Ok(Self(sign.into_boxed_str()))
    }

    pub(super) fn into_wire(self) -> Box<str> {
        self.0
    }
}

/// SystemHelper 32B770's deviceId; not the 64-character hardware-persistence
/// value or the login credential's device ID.
pub struct NativeDeviceId(Box<str>);

impl NativeDeviceId {
    /// Accept a caller's verified native cache observation in the hex subset.
    /// No file lookup, migration, normalization, or cache mutation is performed.
    pub fn from_observed_cache(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > MAX_NATIVE_IDENTITY_BYTES
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::InvalidInput("invalid observed recognition deviceId"));
        }
        Ok(Self(value.into()))
    }

    /// Only for a verified native cache miss. None denotes an observed
    /// TokenUser SID lookup failure, not lack of a Windows observation
    /// provider. An empty adapter list means the native scan yielded nothing.
    /// Rows are supplied in the returned Windows order, without sorting.
    pub fn from_cache_miss(sid: Option<&str>, adapters: &[&[u8]]) -> Result<Self> {
        if sid.is_some_and(|value| !valid_sid(value))
            || adapters.len() > MAX_ADAPTER_ROWS
            || adapters.iter().any(|row| row.len() > 8)
        {
            return Err(Error::InvalidInput(
                "invalid recognition native observations",
            ));
        }
        let mut id = hex::encode_upper(Sha1::digest(sid.unwrap_or(NO_TOKEN_USER_SID).as_bytes()));
        // 32BD50 only requires physical-address length 6; it does not apply
        // the separate clientSign MAC collector's adapter filters.
        if let Some(mac) = adapters.iter().find(|bytes| bytes.len() == 6) {
            id.push_str(&hex::encode_upper(mac));
        }
        Ok(Self(id.into_boxed_str()))
    }

    pub(super) fn into_wire(self) -> Box<str> {
        self.0
    }
}

fn valid_sid(text: &str) -> bool {
    if text.len() > 256 {
        return false;
    }
    let Some(tail) = text.strip_prefix("S-1-") else {
        return false;
    };
    let mut parts = tail.split('-');
    let authority = parts.next().unwrap_or_default();
    let authority_valid = if let Some(hex) = authority.strip_prefix("0x") {
        !hex.is_empty() && hex.len() <= 12 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    } else {
        !authority.is_empty()
            && authority.bytes().all(|byte| byte.is_ascii_digit())
            && authority
                .parse::<u64>()
                .is_ok_and(|value| value < (1_u64 << 48))
    };
    authority_valid
        && parts.enumerate().all(|(index, value)| {
            index < 15
                && !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u32>().is_ok()
        })
}

#[cfg(test)]
mod tests;
