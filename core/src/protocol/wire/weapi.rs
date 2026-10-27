//! Same-origin compatibility envelope verified in report 16. Its standard web
//! RSA/CBC construction was corroborated by the archived pinned reference;
//! this is not a claim that PC EAPI and WEAPI are the same official protocol.
use super::{
    crypto::{encrypt_cbc, random_bytes},
    serialization::compact_json,
};
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use num_bigint::BigUint;
use serde_json::Value;

const IV: &[u8; 16] = b"0102030405060708";
const PRESET: &[u8; 16] = b"0CoJUm6Qyw8W8jud";
const MODULUS: &str = "e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";

/// Contains encrypted request contents, so it deliberately has no Debug.
pub struct Envelope {
    pub params: String,
    pub encrypted_key: String,
}

pub fn encrypt(data: &Value) -> Result<Envelope> {
    let bytes = random_bytes::<8>()?;
    let hex = hex::encode(bytes);
    let key = hex
        .as_bytes()
        .try_into()
        .map_err(|_| Error::Protocol("invalid generated WEAPI key"))?;
    encrypt_with_key(data, key)
}

pub fn encrypt_with_key(data: &Value, key: &[u8; 16]) -> Result<Envelope> {
    let body = compact_json(data)?;
    let inner = STANDARD.encode(encrypt_cbc(body.as_bytes(), PRESET, IV));
    let params = STANDARD.encode(encrypt_cbc(inner.as_bytes(), key, IV));
    let mut reversed = *key;
    reversed.reverse();
    let modulus = BigUint::parse_bytes(MODULUS.as_bytes(), 16)
        .ok_or(Error::Protocol("invalid compiled WEAPI modulus"))?;
    let encrypted_key =
        BigUint::from_bytes_be(&reversed).modpow(&BigUint::from(65537_u32), &modulus);
    Ok(Envelope {
        params,
        encrypted_key: format!("{encrypted_key:0256x}"),
    })
}
