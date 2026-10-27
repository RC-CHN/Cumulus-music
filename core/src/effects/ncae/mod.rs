//! Bounded NCAE decoder, recovered from cloudmusic.dll RVA CEB3B0.
//!
//! Unknown header bytes and binary payloads are preserved. The Rust boundary
//! additionally rejects empty keys, incomplete streams and trailing data.

mod cipher;

use crate::{Error, Result};
use flate2::{Decompress, FlushDecompress, Status};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub container_bytes: usize,
    pub plain_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            container_bytes: 64 * 1024 * 1024,
            plain_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A decoded material; deliberately does not interpret its effect type or JSON.
pub struct Container {
    pub plain: Vec<u8>,
    pub type_id: u16,
    pub opaque_header: [u8; 6],
    pub key_bytes: usize,
    pub compressed_bytes: u32,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub container: &'static str,
    #[serde(rename = "type")]
    pub type_id: u16,
    pub opaque_header_8_13: String,
    pub key_bytes: usize,
    pub compressed_bytes: u32,
    pub plain_bytes: usize,
    pub plain_sha256: String,
}

impl Container {
    pub fn summary(&self) -> Summary {
        Summary {
            container: "NCAE",
            type_id: self.type_id,
            opaque_header_8_13: hex::encode(self.opaque_header),
            key_bytes: self.key_bytes,
            compressed_bytes: self.compressed_bytes,
            plain_bytes: self.plain.len(),
            plain_sha256: hex::encode(Sha256::digest(&self.plain)),
        }
    }
}

pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Container> {
    read(crate::media::open_regular(path)?, limits)
}

/// Reads at most the configured container limit plus one rejection byte.
/// Synchronous file/CPU work belongs on the host's blocking worker.
pub fn read(source: impl Read, limits: Limits) -> Result<Container> {
    let bound = u64::try_from(limits.container_bytes)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::InvalidInput(
            "NCAE container limit exceeds address space",
        ))?;
    let mut data = Vec::new();
    source.take(bound).read_to_end(&mut data)?;
    decode(&data, limits)
}

pub fn decode(data: &[u8], limits: Limits) -> Result<Container> {
    if data.len() > limits.container_bytes {
        return Err(Error::Media("NCAE container exceeds size limit"));
    }
    if data.len() < 22 || &data[..4] != b"NCAE" {
        return Err(Error::Media("invalid or truncated NCAE header"));
    }
    let compressed_bytes = u32::from_le_bytes(data[4..8].try_into().expect("header checked"));
    let count = usize::from(data[16]);
    if count < 5 || !(count - 1).is_multiple_of(4) {
        return Err(Error::Media("invalid NCAE key blob length"));
    }
    let payload_start = 17 + count;
    // Comparing a u64 sum avoids a malicious u32 size overflowing 32-bit hosts.
    if data.len() as u64 != payload_start as u64 + u64::from(compressed_bytes) {
        return Err(Error::Media("NCAE length mismatch"));
    }
    let marker = data[21];
    let key: Vec<_> = data[17..payload_start]
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 4)
        .map(|(_, value)| *value ^ marker)
        .collect();
    let stream = cipher::key_stream(&key)?;
    let mut compressed = data[payload_start..].to_vec();
    cipher::apply(&mut compressed, &stream, 0);
    Ok(Container {
        plain: inflate(&compressed, limits.plain_bytes)?,
        type_id: u16::from_le_bytes(data[14..16].try_into().expect("header checked")),
        opaque_header: data[8..14].try_into().expect("header checked"),
        key_bytes: key.len(),
        compressed_bytes,
    })
}

fn inflate(compressed: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut inflater = Decompress::new(false);
    let mut plain = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let before_in = inflater.total_in();
        let before_out = inflater.total_out();
        // One extra byte detects a size violation, including a zero-byte limit.
        let available = limit.saturating_sub(plain.len()).saturating_add(1);
        let capacity = chunk.len().min(available);
        let status = inflater
            .decompress(
                &compressed[before_in as usize..],
                &mut chunk[..capacity],
                FlushDecompress::None,
            )
            .map_err(|_| Error::Media("invalid NCAE raw DEFLATE payload"))?;
        let written = (inflater.total_out() - before_out) as usize;
        if written > limit.saturating_sub(plain.len()) {
            return Err(Error::Media("NCAE uncompressed payload exceeds size limit"));
        }
        plain.extend_from_slice(&chunk[..written]);
        if status == Status::StreamEnd {
            if inflater.total_in() != compressed.len() as u64 {
                return Err(Error::Media("NCAE trailing compressed bytes"));
            }
            return Ok(plain);
        }
        if inflater.total_in() == before_in && written == 0 {
            return Err(Error::Media("truncated NCAE raw DEFLATE payload"));
        }
    }
}
