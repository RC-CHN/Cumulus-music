//! Official PC 3.1.42 encodeAnonymousId, recovered from cloudmusic.dll.
//!
//! This is a deterministic local encoding, not an anonymous-account receipt or
//! permission. Registration, Cookie scope and verified UID adoption are separate.
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use md5::{Digest, Md5};

/// Local allocation bound; the original std::string implementation has no
/// recovered 4096-byte clamp. Ordinary native device IDs fit within this bound.
pub const MAX_ANONYMOUS_DEVICE_ID_BYTES: usize = 4096;
const KEY: &[u8] = b"3go8&$8*3*3h0k(2)2";

/// Encode valid UTF-8 exactly, preserving case, whitespace and embedded NUL.
///
/// The official bridge converts UTF-16 through cef_string_utf16_to_utf8 before
/// entering the recovered byte-string routine. Rust `&str` cannot represent an
/// isolated UTF-16 surrogate; that CEF conversion edge is outside this API.
pub fn encode_anonymous_id(device_id: &str) -> Result<String> {
    if device_id.len() > MAX_ANONYMOUS_DEVICE_ID_BYTES {
        return Err(Error::InvalidInput(
            "anonymous device ID exceeds local bound",
        ));
    }
    if device_id.is_empty() {
        return Ok(String::new());
    }
    let mixed = device_id
        .bytes()
        .enumerate()
        .map(|(index, byte)| byte ^ KEY[index % KEY.len()])
        .collect::<Vec<_>>();
    // Both native Base64 passes replace '/' and '+' only. Padding '=' remains.
    let inner = URL_SAFE.encode(Md5::digest(mixed));
    let mut payload = Vec::with_capacity(device_id.len() + 1 + inner.len());
    payload.extend_from_slice(device_id.as_bytes());
    payload.push(b' ');
    payload.extend_from_slice(inner.as_bytes());
    Ok(URL_SAFE.encode(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_original_x64_core_vectors_including_unicode_nul_and_md5_blocks() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/anonymous_identity_vectors.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 38);
        for case in cases {
            assert_eq!(
                encode_anonymous_id(case["deviceId"].as_str().unwrap()).unwrap(),
                case["encoded"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn byte_allocation_bound_is_explicit_and_does_not_truncate_or_normalize() {
        assert!(matches!(
            encode_anonymous_id(&"a".repeat(MAX_ANONYMOUS_DEVICE_ID_BYTES + 1)),
            Err(Error::InvalidInput(_))
        ));
        assert!(encode_anonymous_id(&"中".repeat(MAX_ANONYMOUS_DEVICE_ID_BYTES / 3 + 1)).is_err());
    }
}
