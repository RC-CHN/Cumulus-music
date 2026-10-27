//! Native serialData/serialKey/serialData2 and encrypted response decoding.
use super::{
    crypto::{decrypt_ecb, encrypt_ecb},
    serialization::{cache_canonical, compact_json},
};
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use serde_json::Value;
use subtle::ConstantTimeEq;

pub const CACHE_KEY: &[u8; 16] = b")(13daqP@ssw0rd~";
pub const EAPI_KEY: &[u8; 16] = b"e82ckenh8dichen8";
pub const SERIAL_DATA2_KEY: &[u8; 16] = b"rFgB&h#%2?^eDg:Q";
const SEPARATOR: &str = "-36cd479b6b5-";

pub fn md5_hex(data: &[u8]) -> String {
    hex::encode(Md5::digest(data))
}

pub fn serial_key(text: &str) -> Result<String> {
    Ok(STANDARD.encode(encrypt_ecb(text.as_bytes(), CACHE_KEY)?))
}

pub fn cache_envelope(data: &Value) -> Result<(String, String)> {
    let canonical = cache_canonical(data)?;
    let key = serial_key(&canonical)?;
    Ok((canonical, key))
}

pub fn encrypt(path: &str, data: &Value) -> Result<String> {
    encrypt_json(path, &compact_json(data)?)
}

/// Accept already serialized JSON to preserve the exact native input bytes.
pub fn encrypt_json(path: &str, body: &str) -> Result<String> {
    if path.contains(SEPARATOR) {
        return Err(Error::InvalidInput("ambiguous EAPI path"));
    }
    let digest = md5_hex(format!("nobody{path}use{body}md5forencrypt").as_bytes());
    let plain = format!("{path}{SEPARATOR}{body}{SEPARATOR}{digest}");
    Ok(hex::encode_upper(encrypt_ecb(plain.as_bytes(), EAPI_KEY)?))
}

pub fn unwrap(params: &str) -> Result<(String, Value)> {
    let cipher = hex::decode(params).map_err(|_| Error::Protocol("invalid EAPI hex"))?;
    let plain = decrypt_ecb(&cipher, EAPI_KEY)?;
    let text = std::str::from_utf8(&plain).map_err(|_| Error::Protocol("invalid EAPI text"))?;
    let (path, rest) = text
        .split_once(SEPARATOR)
        .ok_or(Error::Protocol("missing EAPI path separator"))?;
    let (body, digest) = rest
        .rsplit_once(SEPARATOR)
        .ok_or(Error::Protocol("missing EAPI digest separator"))?;
    let expected = md5_hex(format!("nobody{path}use{body}md5forencrypt").as_bytes());
    if !bool::from(expected.as_bytes().ct_eq(digest.as_bytes())) {
        return Err(Error::Protocol("EAPI digest mismatch"));
    }
    Ok((path.into(), serde_json::from_str(body)?))
}

pub fn serial_data2(data: &[u8]) -> Result<String> {
    if data.is_empty() {
        return Err(Error::InvalidInput("serialData2 requires nonempty input"));
    }
    Ok(hex::encode_upper(encrypt_ecb(data, SERIAL_DATA2_KEY)?))
}

/// Native response has no request digest envelope. Validate every PKCS7 byte
/// (stricter than the original routine, which used only the final byte).
pub fn decrypt_response(raw: &[u8]) -> Result<Vec<u8>> {
    decrypt_ecb(raw, EAPI_KEY)
}
