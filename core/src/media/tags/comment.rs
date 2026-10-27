use crate::{
    Error, Result,
    protocol::wire::crypto::{decrypt_ecb, encrypt_ecb},
};
use base64::{Engine, engine::general_purpose::STANDARD};

// cloudmusic.dll x64 329440 -> 329560 -> 62C5D0; also NCM metadata.
pub const COMMENT_PREFIX: &str = "163 key(Don't modify):";
pub(crate) const COMMENT_KEY: &[u8; 16] = b"#14ljk_!\\]&0U<'(";
const LIMIT: usize = 16 * 1024 * 1024;
pub fn encode_comment(plain: &str) -> Result<String> {
    if plain.len() > LIMIT {
        return Err(Error::InvalidInput("comment exceeds limit"));
    }
    Ok(format!(
        "{COMMENT_PREFIX}{}",
        STANDARD.encode(encrypt_ecb(plain.as_bytes(), COMMENT_KEY)?)
    ))
}
pub fn decode_comment(encoded: &str) -> Result<String> {
    if encoded.len() > LIMIT * 4 / 3 + 128 {
        return Err(Error::Media("encoded comment exceeds limit"));
    }
    let body = encoded
        .strip_prefix(COMMENT_PREFIX)
        .ok_or(Error::Media("invalid comment prefix"))?;
    let bytes = STANDARD
        .decode(body)
        .map_err(|_| Error::Media("invalid comment base64"))?;
    let plain =
        decrypt_ecb(&bytes, COMMENT_KEY).map_err(|_| Error::Media("invalid comment ciphertext"))?;
    if plain.len() > LIMIT {
        return Err(Error::Media("decoded comment exceeds limit"));
    }
    String::from_utf8(plain).map_err(|_| Error::Media("invalid comment UTF-8"))
}
