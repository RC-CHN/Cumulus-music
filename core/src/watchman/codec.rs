use super::SdkBuild;
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};

pub const MAX_BYTES: usize = 1024 * 1024;
const MASK: [u8; 6] = [31, 125, 244, 60, 32, 48];
const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_text(text: &str) -> Result<String> {
    if text.len() > MAX_BYTES {
        return Err(Error::InvalidInput("Watchman text exceeds limit"));
    }
    Ok(hex::encode(
        text.bytes()
            .enumerate()
            .map(|(i, b)| 0_u8.wrapping_sub(b ^ MASK[i % MASK.len()]))
            .collect::<Vec<_>>(),
    ))
}
pub fn decode_text(encoded: &str) -> Result<String> {
    if encoded.len() > MAX_BYTES * 2 {
        return Err(Error::InvalidInput("Watchman text exceeds limit"));
    }
    let bytes = hex::decode(encoded).map_err(|_| Error::Protocol("invalid Watchman text hex"))?;
    String::from_utf8(
        bytes
            .into_iter()
            .enumerate()
            .map(|(i, b)| 0_u8.wrapping_sub(b) ^ MASK[i % MASK.len()])
            .collect(),
    )
    .map_err(|_| Error::Protocol("invalid Watchman text UTF-8"))
}

/// bc(): local wall clock only; no server clock correction. Randomness is
/// explicit for reproducibility; a host uses its operating-system random source.
pub fn request_nonce(timestamp_ms: u64, random: [u8; 8]) -> String {
    let mut woven = [0_u8; 16];
    for (i, (t, r)) in timestamp_ms
        .to_be_bytes()
        .into_iter()
        .zip(random)
        .enumerate()
    {
        for (half, shift) in [4, 0].into_iter().enumerate() {
            for bit in 0..4 {
                woven[i * 2 + half] |= ((r >> (shift + bit)) & 1) << (bit * 2)
                    | ((t >> (shift + bit)) & 1) << (bit * 2 + 1);
            }
        }
    }
    let digest = Md5::digest(format!(
        "{}dAWsBhCqtOaNLLJ25hBzWbqWXwiK99Wd",
        hex::encode(woven)
    ));
    STANDARD.encode([&digest[..8], &woven].concat())
}
fn key(build: SdkBuild, seed: &[u8]) -> [u8; 64] {
    let secret = build.secret();
    std::array::from_fn(|i| secret[i % secret.len()] ^ seed[i % 4])
}
fn transform(build: SdkBuild, b: u8, i: usize) -> u8 {
    match build {
        SdkBuild::Af2952a4 => {
            (b.wrapping_sub(128) ^ (-54_i32 + i as i32) as u8).wrapping_add((32 + i) as u8)
        }
        SdkBuild::B602a5ad7 => {
            (b ^ (91_i32 - i as i32) as u8).wrapping_add((-70_i32 + i as i32) as u8)
        }
    }
}
fn inverse_transform(build: SdkBuild, b: u8, i: usize) -> u8 {
    match build {
        SdkBuild::Af2952a4 => {
            (b.wrapping_sub((32 + i) as u8) ^ (-54_i32 + i as i32) as u8).wrapping_add(128)
        }
        SdkBuild::B602a5ad7 => {
            b.wrapping_sub((-70_i32 + i as i32) as u8) ^ (91_i32 - i as i32) as u8
        }
    }
}
/// ib(): explicit serialized fields and seed; never fabricates environment data.
pub fn encode_packet(payload: &[u8], seed: [u8; 4]) -> Result<String> {
    encode_packet_for(SdkBuild::Af2952a4, payload, seed)
}
pub fn encode_packet_for(build: SdkBuild, payload: &[u8], seed: [u8; 4]) -> Result<String> {
    if payload.len() > MAX_BYTES {
        return Err(Error::InvalidInput("Watchman packet exceeds limit"));
    }
    let mut data = payload.to_vec();
    data.extend_from_slice(format!("{:08x}", crc32fast::hash(payload)).as_bytes());
    let length = data.len();
    data.resize((length + 4).div_ceil(64) * 64 - 4, 0);
    data.extend_from_slice(&(length as u32).to_be_bytes());
    let k = key(build, &seed);
    let sbox = build.substitution();
    let mut previous = k;
    let mut output = seed.to_vec();
    for block in data.chunks_exact(64) {
        let next = std::array::from_fn::<_, 64, _>(|i| {
            let mixed =
                (transform(build, block[i], i) ^ k[i]).wrapping_add(previous[i]) ^ previous[i];
            sbox[sbox[mixed as usize] as usize]
        });
        output.extend_from_slice(&next);
        previous = next;
    }
    Ok(STANDARD
        .encode(output)
        .bytes()
        .map(|b| {
            if b == b'=' {
                build.padding() as char
            } else {
                build.alphabet()[STANDARD_ALPHABET.iter().position(|x| *x == b).unwrap()] as char
            }
        })
        .collect())
}
pub fn decode_packet(encoded: &str) -> Result<Vec<u8>> {
    decode_packet_for(SdkBuild::Af2952a4, encoded)
}
pub fn decode_packet_for(build: SdkBuild, encoded: &str) -> Result<Vec<u8>> {
    if encoded.len() > (MAX_BYTES + 80).div_ceil(3) * 4 {
        return Err(Error::InvalidInput("Watchman packet exceeds limit"));
    }
    let standard: Result<Vec<u8>> = encoded
        .bytes()
        .map(|b| {
            if b == build.padding() {
                Ok(b'=')
            } else {
                build
                    .alphabet()
                    .iter()
                    .position(|x| *x == b)
                    .map(|i| STANDARD_ALPHABET[i])
                    .ok_or(Error::Protocol("invalid Watchman alphabet"))
            }
        })
        .collect();
    let bytes = STANDARD
        .decode(standard?)
        .map_err(|_| Error::Protocol("invalid Watchman base64"))?;
    if bytes.len() < 68 || (bytes.len() - 4) % 64 != 0 {
        return Err(Error::Protocol("invalid Watchman block length"));
    }
    let mut inverse = [0_u8; 256];
    for (i, b) in build.substitution().iter().enumerate() {
        inverse[*b as usize] = i as u8;
    }
    let k = key(build, &bytes[..4]);
    let mut previous = k;
    let mut content = Vec::with_capacity(bytes.len() - 4);
    for block in bytes[4..].chunks_exact(64) {
        for i in 0..64 {
            let mixed = inverse[inverse[block[i] as usize] as usize];
            let t = (mixed ^ previous[i]).wrapping_sub(previous[i]) ^ k[i];
            content.push(inverse_transform(build, t, i));
        }
        previous.copy_from_slice(block);
    }
    let size = content.len();
    let length = u32::from_be_bytes(content[size - 4..].try_into().unwrap()) as usize;
    if length < 8
        || length > size - 4
        || length - 8 > MAX_BYTES
        || (length + 4).div_ceil(64) * 64 != size
        || content[length..size - 4].iter().any(|b| *b != 0)
    {
        return Err(Error::Protocol("invalid Watchman length or padding"));
    }
    let payload = &content[..length - 8];
    if content[length - 8..length] != *format!("{:08x}", crc32fast::hash(payload)).as_bytes() {
        return Err(Error::Protocol("Watchman CRC32 mismatch"));
    }
    Ok(payload.to_vec())
}
