use super::{decode_record, valid_persistent_id};
use crate::{Error, Result};
use md5::{Digest, Md5};
use serde::Serialize;
use std::{collections::BTreeMap, fs, io::Read, path::Path};
use subtle::ConstantTimeEq;

pub const ID_RECORD_KEY: u32 = 0x90001;
pub const MARKER_RECORD_KEY: u32 = 0x1001;
pub const MARKER: &[u8] = b"b6dec0e1-fa12-4698-a3e0-321498e274284b5b1d41b26801663ffe0306cfbef394";
pub const MAX_CONTAINER_BYTES: usize = 16 * 1024 * 1024;
const VERSION: u32 = 0x003c0000;
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated index"),
    )
}

/// Strict, non-destructive PFDW parser. Unknown payloads remain opaque; a
/// hardware-match record is raw bytes, not necessarily an encrypted record.
pub fn parse_container(bytes: &[u8]) -> Result<BTreeMap<u32, &[u8]>> {
    if bytes.len() < 28
        || bytes.len() > MAX_CONTAINER_BYTES
        || &bytes[..4] != b"PFDW"
        || word(bytes, 4) != VERSION
    {
        return Err(Error::Protocol("invalid device container header"));
    }
    if !bool::from(bytes[8..24].ct_eq(Md5::digest(&bytes[24..]).as_slice())) {
        return Err(Error::Protocol("device container digest mismatch"));
    }
    let count = word(bytes, 24) as usize;
    if count > (bytes.len() - 28) / 12 || count > 4096 {
        return Err(Error::Protocol("invalid device record index"));
    }
    let start = 28 + count * 12;
    let mut result = BTreeMap::new();
    let mut ranges = Vec::new();
    for i in 0..count {
        let row = 28 + i * 12;
        let key = word(bytes, row);
        let offset = word(bytes, row + 4) as usize;
        let size = word(bytes, row + 8) as usize;
        if offset < start
            || offset > bytes.len()
            || size > bytes.len() - offset
            || result.contains_key(&key)
        {
            return Err(Error::Protocol("invalid or duplicate device record"));
        }
        result.insert(key, &bytes[offset..offset + size]);
        ranges.push((offset, offset + size));
    }
    ranges.sort_unstable();
    let mut end = start;
    for (begin, next) in ranges {
        if begin < end {
            return Err(Error::Protocol("overlapping device records"));
        }
        end = next;
    }
    Ok(result)
}

/// Canonical key-order container writer. It does not reproduce the native
/// append-in-place layout or write to Windows system paths.
pub fn encode_container(records: &BTreeMap<u32, Vec<u8>>) -> Result<Vec<u8>> {
    if records.len() > 4096 {
        return Err(Error::InvalidInput("too many device records"));
    }
    let start = 28 + records.len() * 12;
    let total = records
        .values()
        .try_fold(start, |n, v| n.checked_add(v.len()))
        .filter(|n| *n <= MAX_CONTAINER_BYTES)
        .ok_or(Error::InvalidInput("device container too large"))?;
    let mut out = vec![0; start];
    out[..4].copy_from_slice(b"PFDW");
    out[4..8].copy_from_slice(&VERSION.to_le_bytes());
    out[24..28].copy_from_slice(&(records.len() as u32).to_le_bytes());
    out.reserve(total - start);
    for (i, (key, data)) in records.iter().enumerate() {
        let row = 28 + i * 12;
        let offset = out.len() as u32;
        out[row..row + 4].copy_from_slice(&key.to_le_bytes());
        out[row + 4..row + 8].copy_from_slice(&offset.to_le_bytes());
        out[row + 8..row + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend(data);
    }
    let digest = Md5::digest(&out[24..]);
    out[8..24].copy_from_slice(&digest);
    Ok(out)
}
pub fn read_container(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_CONTAINER_BYTES as u64 {
        return Err(Error::InvalidInput(
            "device file must be a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((MAX_CONTAINER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    parse_container(&bytes)?;
    Ok(bytes)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecordSummary {
    pub key: u32,
    pub stored_bytes: usize,
    pub plaintext_bytes: Option<usize>,
    pub digest_valid: Option<bool>,
    pub valid_id_shape: Option<bool>,
    pub derived_suffix_valid: Option<bool>,
    pub known_marker: Option<bool>,
}
pub fn inspect_container(bytes: &[u8]) -> Result<Vec<DeviceRecordSummary>> {
    parse_container(bytes)?
        .into_iter()
        .map(|(key, data)| {
            let plain = if [ID_RECORD_KEY, MARKER_RECORD_KEY].contains(&key) {
                Some(decode_record(data)?)
            } else {
                None
            };
            let id = key == ID_RECORD_KEY;
            let marker = key == MARKER_RECORD_KEY;
            Ok(DeviceRecordSummary {
                key,
                stored_bytes: data.len(),
                plaintext_bytes: plain.as_ref().map(Vec::len),
                digest_valid: plain.as_ref().map(|_| true),
                valid_id_shape: id.then(|| {
                    plain
                        .as_ref()
                        .is_some_and(|p| std::str::from_utf8(p).is_ok_and(valid_persistent_id))
                }),
                derived_suffix_valid: id.then(|| {
                    plain.as_ref().is_some_and(|p| {
                        p.len() == 64
                            && bool::from(
                                p[32..].ct_eq(hex::encode(Md5::digest(&p[..32])).as_bytes()),
                            )
                    })
                }),
                known_marker: marker.then(|| plain.as_deref() == Some(MARKER)),
            })
        })
        .collect()
}
