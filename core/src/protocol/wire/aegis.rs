//! SDK B/S/R reconstruction. Pure algorithms can be compared offline;
//! successful business interoperability was NOT established in the research.
use super::{
    crypto::{decrypt_ecb, encrypt_ecb, random_bytes},
    serialization::encode_component,
};
use crate::{Error, Result};
use aes_gcm::{
    Aes128Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

pub struct EnvelopeContext<'a> {
    pub static_key: &'a [u8],
    pub dynamic_key: &'a [u8],
    pub public_key: &'a [u8; 32],
    pub version: &'a str,
    pub os_name: &'a str,
    pub sk: &'a str,
    pub session_id: &'a str,
}

pub struct Randomness {
    pub mask: [u8; 16],
    pub ephemeral_private: [u8; 32],
    pub iv: [u8; 12],
}

impl Randomness {
    pub fn generate() -> Result<Self> {
        Ok(Self {
            mask: random_bytes()?,
            ephemeral_private: random_bytes()?,
            iv: random_bytes()?,
        })
    }
}

/// SDK 97130: XOR each byte, Base64, rotate left, prefix the raw mask.
pub fn mix(ciphertext: &[u8], mask: &[u8; 16]) -> Vec<u8> {
    let mixed: Vec<_> = ciphertext
        .iter()
        .enumerate()
        .map(|(i, c)| c ^ mask[i % 16])
        .collect();
    let mut encoded = STANDARD.encode(mixed).into_bytes();
    if !encoded.is_empty() {
        let shift = usize::from(mask[0] & 15) % encoded.len();
        encoded.rotate_left(shift)
    }
    let mut output = mask.to_vec();
    output.extend(encoded);
    output
}

pub fn unmix(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 16 {
        return Err(Error::Protocol("truncated Aegis mixed data"));
    }
    let (mask, encoded) = data.split_at(16);
    let mut encoded = encoded.to_vec();
    if !encoded.is_empty() {
        let shift = usize::from(mask[0] & 15) % encoded.len();
        encoded.rotate_right(shift)
    }
    let mut decoded = STANDARD
        .decode(encoded)
        .map_err(|_| Error::Protocol("invalid Aegis Base64"))?;
    for (i, c) in decoded.iter_mut().enumerate() {
        *c ^= mask[i % 16]
    }
    Ok(decoded)
}

pub fn business_encrypt(
    body: &[u8],
    static_key: &[u8],
    dynamic_key: &[u8],
    mask: &[u8; 16],
) -> Result<Vec<u8>> {
    encrypt_ecb(&mix(&encrypt_ecb(body, static_key)?, mask), dynamic_key)
}

pub fn business_decrypt(body: &[u8], static_key: &[u8], dynamic_key: &[u8]) -> Result<Vec<u8>> {
    decrypt_ecb(&unmix(&decrypt_ecb(body, dynamic_key)?)?, static_key)
}

/// SDK 83CC0/91E50: ephemeral public key || IV || ciphertext || GCM tag.
pub fn seal_dynamic(
    dynamic_key: &[u8],
    public_key: &[u8; 32],
    os_name: &str,
    sk: &str,
    ephemeral_private: &[u8; 32],
    iv: &[u8; 12],
) -> Result<Vec<u8>> {
    let private = StaticSecret::from(*ephemeral_private);
    let epk = PublicKey::from(&private);
    let shared = private.diffie_hellman(&PublicKey::from(*public_key));
    if !shared.was_contributory() {
        return Err(Error::Protocol("noncontributory Aegis public key"));
    }
    let mut key = [0_u8; 16];
    Hkdf::<Sha256>::new(Some(&[0; 32]), shared.as_bytes())
        .expand(epk.as_bytes(), &mut key)
        .map_err(|_| Error::Protocol("invalid Aegis HKDF output"))?;
    let plain = format!("{}|{os_name}|{sk}", STANDARD.encode(dynamic_key));
    let cipher =
        Aes128Gcm::new_from_slice(&key).map_err(|_| Error::Protocol("invalid Aegis GCM key"))?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(iv), plain.as_bytes())
        .map_err(|_| Error::Protocol("Aegis GCM encryption failed"))?;
    let mut out = epk.as_bytes().to_vec();
    out.extend(iv);
    out.extend(sealed);
    Ok(out)
}

pub fn envelope(body: &[u8], context: &EnvelopeContext<'_>) -> Result<String> {
    envelope_with_randomness(body, context, &Randomness::generate()?)
}

pub fn envelope_with_randomness(
    body: &[u8],
    context: &EnvelopeContext<'_>,
    randomness: &Randomness,
) -> Result<String> {
    let b = business_encrypt(
        body,
        context.static_key,
        context.dynamic_key,
        &randomness.mask,
    )?;
    let s = seal_dynamic(
        context.dynamic_key,
        context.public_key,
        context.os_name,
        context.sk,
        &randomness.ephemeral_private,
        &randomness.iv,
    )?;
    let r = encrypt_ecb(
        format!("{}|{}", context.version, context.session_id).as_bytes(),
        context.static_key,
    )?;
    Ok([("B", b), ("S", s), ("R", r)]
        .into_iter()
        .map(|(name, value)| format!("{name}={}", encode_component(&STANDARD.encode(value))))
        .collect::<Vec<_>>()
        .join("&"))
}

/// SDK 92FF0/97060 uses the Base64 TEXT as the HMAC key, not decoded bytes.
pub fn key_signature(sign_key: &[u8], timestamp: &str, nonce: &str) -> Result<String> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(sign_key)
        .map_err(|_| Error::Protocol("invalid Aegis signing key"))?;
    mac.update(timestamp.as_bytes());
    mac.update(nonce.as_bytes());
    Ok(STANDARD.encode(mac.finalize().into_bytes()))
}

pub fn verify_key_response(data: &Value, sign_key: &[u8], nonce: &str) -> Result<bool> {
    let timestamp = match &data["timestamp"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(Error::Response("missing Aegis timestamp")),
    };
    let signature = data["signature"]
        .as_str()
        .ok_or(Error::Response("missing Aegis signature"))?;
    let Ok(bytes) = STANDARD.decode(signature) else {
        return Ok(false);
    };
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(sign_key)
        .map_err(|_| Error::Protocol("invalid Aegis signing key"))?;
    mac.update(timestamp.as_bytes());
    mac.update(nonce.as_bytes());
    Ok(mac.verify_slice(&bytes).is_ok())
}

pub fn decode_key_response(data: &Value, static_key: &[u8]) -> Result<Value> {
    let encoded = data["encryptedData"]
        .as_str()
        .ok_or(Error::Response("missing Aegis encrypted key data"))?;
    let cipher = STANDARD
        .decode(encoded)
        .map_err(|_| Error::Protocol("invalid Aegis encrypted key Base64"))?;
    Ok(serde_json::from_slice(&decrypt_ecb(&cipher, static_key)?)?)
}
