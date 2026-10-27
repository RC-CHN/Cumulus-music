//! Official za(): x64 Murmur-style fingerprint over JavaScript UTF-16 units.
//! The block path masks to bytes, while the tail keeps entire code units.
//! This is deliberately not a generic UTF-8 MurmurHash implementation.
use super::{MAX_BYTES, Observation};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host facts only; no Debug implementation exposes a fingerprint preimage.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FingerprintReading {
    Fingerprint { preimage: String },
    Unsupported,
    Failed,
}

pub fn fingerprint_observation(key: &str, reading: FingerprintReading) -> Result<Observation> {
    if !matches!(key, "Jb" | "Qa" | "dc" | "jb") {
        return Err(Error::InvalidInput("not a Watchman fingerprint field"));
    }
    let (value, failed) = match reading {
        FingerprintReading::Fingerprint { preimage } => {
            if preimage.len() > MAX_BYTES {
                return Err(Error::InvalidInput(
                    "Watchman fingerprint preimage exceeds limit",
                ));
            }
            (Value::String(fingerprint_hash(&preimage, 0)), false)
        }
        FingerprintReading::Unsupported => (Value::String(String::new()), false),
        FingerprintReading::Failed => (Value::Null, true),
    };
    Ok(Observation {
        key: key.into(),
        value,
        failed,
    })
}

fn avalanche(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^ (value >> 33)
}

/// A preimage comes from actual host measurements, not synthetic identity data.
/// Empty input follows the SDK's `input || "~"` rule. Seed is zero for all
/// recovered device collectors; it remains explicit for official comparison.
pub fn fingerprint_hash(input: &str, seed: u32) -> String {
    let input = if input.is_empty() { "~" } else { input };
    let units: Vec<_> = input.encode_utf16().collect();
    let c1 = 0x87c37b91114253d5u64;
    let c2 = 0x4cf5ad432745937fu64;
    let mut h1 = u64::from(seed);
    let mut h2 = u64::from(seed);
    let mut chunks = units.chunks_exact(16);
    for chunk in &mut chunks {
        let word = |start: usize| {
            chunk[start..start + 8]
                .iter()
                .enumerate()
                .fold(0u64, |n, (i, v)| n | (u64::from(v & 255) << (8 * i)))
        };
        let k1 = word(0).wrapping_mul(c1).rotate_left(31).wrapping_mul(c2);
        h1 ^= k1;
        h1 = h1.rotate_left(27).wrapping_add(h2);
        h1 = h1.wrapping_mul(5).wrapping_add(0x52dce729);
        let k2 = word(8).wrapping_mul(c2).rotate_left(33).wrapping_mul(c1);
        h2 ^= k2;
        h2 = h2.rotate_left(31).wrapping_add(h1);
        h2 = h2.wrapping_mul(5).wrapping_add(0x38495ab5);
    }
    let tail = chunks.remainder();
    let word = |slice: &[u16]| {
        slice.iter().enumerate().fold(0u64, |n, (i, v)| {
            n ^ u64::from(*v).wrapping_shl((8 * i) as u32)
        })
    };
    if tail.len() > 8 {
        h2 ^= word(&tail[8..])
            .wrapping_mul(c2)
            .rotate_left(33)
            .wrapping_mul(c1);
    }
    if !tail.is_empty() {
        h1 ^= word(&tail[..tail.len().min(8)])
            .wrapping_mul(c1)
            .rotate_left(31)
            .wrapping_mul(c2);
    }
    h1 ^= units.len() as u64;
    h2 ^= units.len() as u64;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = avalanche(h1);
    h2 = avalanche(h2);
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    format!("{h1:016x}{h2:016x}")
}
