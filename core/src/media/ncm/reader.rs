use super::{CORE_KEY, KEY_PREFIX, MAGIC, MAX_COVER, MAX_FIELD, META_KEY, META_PREFIX, cipher};
use crate::{Error, Result, protocol::wire::crypto::decrypt_ecb};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPolicy {
    Compatible,
    Strict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    Mp3,
    M4a,
}

#[derive(Clone, Debug, Serialize)]
pub struct NcmInfo {
    pub audio_format: Option<AudioFormat>,
    pub audio_bytes: u64,
    pub audio_offset: u64,
    pub metadata: Option<Value>,
    pub cover_bytes: u64,
    pub cover_frame_bytes: u32,
    pub crc_verified: bool,
    pub crc32_field: u32,
    pub reserved: [u8; 2],
    pub image_version: u8,
}

pub struct NcmReader<R> {
    source: R,
    info: NcmInfo,
    cover: Vec<u8>,
    mask: [u8; 256],
    position: u64,
}

impl NcmReader<File> {
    pub fn open(path: impl AsRef<Path>, policy: HeaderPolicy) -> Result<Self> {
        let source = crate::media::open_regular(path)?;
        Self::new(source, policy)
    }
}

fn exact<R: Read + Seek>(source: &mut R, total: u64, count: u64) -> Result<Vec<u8>> {
    let position = source.stream_position()?;
    if count > total.saturating_sub(position) {
        return Err(Error::Media("truncated NCM field"));
    }
    let size =
        usize::try_from(count).map_err(|_| Error::Media("NCM field exceeds address space"))?;
    let mut bytes = vec![0; size];
    source.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn number<R: Read + Seek>(source: &mut R, total: u64) -> Result<u32> {
    Ok(u32::from_le_bytes(
        exact(source, total, 4)?.try_into().expect("four bytes"),
    ))
}
fn sized<R: Read + Seek>(
    source: &mut R,
    total: u64,
    crc: &mut crc32fast::Hasher,
) -> Result<Vec<u8>> {
    let length = number(source, total)?;
    crc.update(&length.to_le_bytes());
    if u64::from(length) > MAX_FIELD {
        return Err(Error::Media("NCM field exceeds size limit"));
    }
    let bytes = exact(source, total, u64::from(length))?;
    crc.update(&bytes);
    Ok(bytes)
}

impl<R: Read + Seek> NcmReader<R> {
    pub fn new(mut source: R, policy: HeaderPolicy) -> Result<Self> {
        let total = source.seek(SeekFrom::End(0))?;
        source.rewind()?;
        let prefix = exact(&mut source, total, 10)?;
        if &prefix[..8] != MAGIC {
            return Err(Error::Media("unsupported NCM magic"));
        }
        let reserved = [prefix[8], prefix[9]];
        let mut crc = crc32fast::Hasher::new();
        crc.update(&prefix);
        let mut key = sized(&mut source, total, &mut crc)?;
        for byte in &mut key {
            *byte ^= 0x64;
        }
        let key = decrypt_ecb(&key, CORE_KEY).map_err(|_| Error::Media("invalid NCM key field"))?;
        let key = key
            .strip_prefix(KEY_PREFIX)
            .filter(|v| !v.is_empty())
            .ok_or(Error::Media("invalid NCM audio key prefix"))?;
        let mask = cipher::key_stream(key)?;
        let mut wrapped = sized(&mut source, total, &mut crc)?;
        let metadata = if wrapped.is_empty() {
            None
        } else {
            for byte in &mut wrapped {
                *byte ^= 0x63;
            }
            let payload = wrapped
                .strip_prefix(META_PREFIX)
                .ok_or(Error::Media("invalid NCM metadata wrapper"))?;
            let encrypted = STANDARD
                .decode(payload)
                .map_err(|_| Error::Media("invalid NCM metadata base64"))?;
            let plain = decrypt_ecb(&encrypted, META_KEY)
                .map_err(|_| Error::Media("invalid NCM metadata cipher"))?;
            let plain = plain
                .strip_prefix(b"music:")
                .ok_or(Error::Media("invalid NCM metadata prefix"))?;
            let value: Value = serde_json::from_slice(plain)
                .map_err(|_| Error::Media("invalid NCM metadata JSON"))?;
            if !value.is_object() {
                return Err(Error::Media("NCM metadata must be an object"));
            }
            Some(value)
        };
        let crc32_field = number(&mut source, total)?;
        let crc_verified = crc.finalize() == crc32_field;
        let image_version = exact(&mut source, total, 1)?[0];
        if policy == HeaderPolicy::Strict
            && (!crc_verified || reserved[0] != 1 || image_version != 1)
        {
            return Err(Error::Media("NCM header version or CRC mismatch"));
        }
        let cover_frame_bytes = number(&mut source, total)?;
        let cover_bytes = number(&mut source, total)?;
        if cover_bytes > cover_frame_bytes || u64::from(cover_bytes) > MAX_COVER {
            return Err(Error::Media("invalid NCM cover size"));
        }
        let cover_start = source.stream_position()?;
        if u64::from(cover_frame_bytes) > total.saturating_sub(cover_start) {
            return Err(Error::Media("truncated NCM cover frame"));
        }
        let cover = exact(&mut source, total, u64::from(cover_bytes))?;
        let audio_offset = cover_start + u64::from(cover_frame_bytes);
        source.seek(SeekFrom::Start(audio_offset))?;
        let audio_bytes = total - audio_offset;
        if audio_bytes == 0 {
            return Err(Error::Media("empty NCM audio payload"));
        }
        let info = NcmInfo {
            audio_format: None,
            audio_bytes,
            audio_offset,
            metadata,
            cover_bytes: u64::from(cover_bytes),
            cover_frame_bytes,
            crc_verified,
            crc32_field,
            reserved,
            image_version,
        };
        let mut reader = Self {
            source,
            info,
            cover,
            mask,
            position: 0,
        };
        let mut prefix = [0u8; 16];
        let count = usize::try_from(audio_bytes.min(16)).expect("small signature");
        reader.read_exact(&mut prefix[..count])?;
        reader.rewind()?;
        reader.info.audio_format = detect(&prefix[..count]);
        Ok(reader)
    }
    pub fn info(&self) -> &NcmInfo {
        &self.info
    }
    pub fn cover(&self) -> &[u8] {
        &self.cover
    }
    pub fn into_inner(self) -> R {
        self.source
    }
}

fn detect(bytes: &[u8]) -> Option<AudioFormat> {
    if bytes.starts_with(b"fLaC") {
        Some(AudioFormat::Flac)
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some(AudioFormat::M4a)
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 && bytes[1] & 6 != 0)
    {
        Some(AudioFormat::Mp3)
    } else {
        None
    }
}

impl<R: Read> Read for NcmReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.info.audio_bytes - self.position;
        let count = buf
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        if count == 0 {
            return Ok(0);
        }
        let count = self.source.read(&mut buf[..count])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "NCM audio was truncated",
            ));
        }
        cipher::apply(&mut buf[..count], &self.mask, self.position);
        self.position += count as u64;
        Ok(count)
    }
}
impl<R: Seek> Seek for NcmReader<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.info.audio_bytes) + i128::from(n),
        };
        if position < 0 || position > i128::from(self.info.audio_bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside NCM audio payload",
            ));
        }
        let position = position as u64;
        self.source
            .seek(SeekFrom::Start(self.info.audio_offset + position))?;
        self.position = position;
        Ok(position)
    }
}
