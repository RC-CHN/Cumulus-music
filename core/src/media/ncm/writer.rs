use super::{CORE_KEY, KEY_PREFIX, MAGIC, MAX_FIELD, META_KEY, META_PREFIX, cipher, export};
use crate::{
    Error, Result,
    media::validate_cover,
    protocol::wire::crypto::{encrypt_ecb, random_bytes},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::Md5;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Default)]
pub struct NcmWriteOptions<'a> {
    pub metadata: Option<&'a Value>,
    pub cover: &'a [u8],
    /// None generates a new 32-byte key. Explicit keys are useful for format
    /// interoperability tests; this cipher is not an authorization mechanism.
    pub audio_key: Option<&'a [u8]>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NcmWriteReport {
    pub audio_bytes: u64,
    pub audio_md5: String,
    pub audio_sha256: String,
    pub header_crc32: u32,
    pub cover_bytes: usize,
}

fn append_field(header: &mut Vec<u8>, field: &[u8]) -> Result<()> {
    if field.len() as u64 > MAX_FIELD {
        return Err(Error::InvalidInput("NCM field exceeds size limit"));
    }
    header.extend_from_slice(&(field.len() as u32).to_le_bytes());
    header.extend_from_slice(field);
    Ok(())
}

/// Writes to a caller-owned staging stream. On error the caller must discard
/// that partial stream. Use write_file for automatic cleanup and publication.
pub fn write_stream(
    source: &mut impl Read,
    output: &mut impl Write,
    options: NcmWriteOptions<'_>,
) -> Result<NcmWriteReport> {
    write_inner(source, output, options, None)
}

/// Preserve M/23's exact comment bytes, including its numeric docId spelling.
pub fn write_plan_stream(
    source: &mut impl Read,
    output: &mut impl Write,
    plan: &crate::media::tags::TagPlan,
    cover: &[u8],
) -> Result<NcmWriteReport> {
    if !plan.encrypted || !plan.comment_plain.starts_with("music:") {
        return Err(Error::InvalidInput(
            "NCM requires an encrypted music tag plan",
        ));
    }
    write_inner(
        source,
        output,
        NcmWriteOptions {
            cover,
            ..Default::default()
        },
        Some(&plan.comment_plain),
    )
}

fn write_inner(
    source: &mut impl Read,
    output: &mut impl Write,
    options: NcmWriteOptions<'_>,
    comment: Option<&str>,
) -> Result<NcmWriteReport> {
    if !options.cover.is_empty() {
        validate_cover(options.cover)?;
    }
    let random = if options.audio_key.is_none() {
        Some(random_bytes::<32>()?)
    } else {
        None
    };
    let key = options
        .audio_key
        .or_else(|| random.as_ref().map(|v| v.as_slice()))
        .expect("key provided or generated");
    if key.is_empty() || key.len() > 256 {
        return Err(Error::InvalidInput(
            "NCM audio key must have 1 to 256 bytes",
        ));
    }
    let mask = cipher::key_stream(key)?;
    let mut plain_key = KEY_PREFIX.to_vec();
    plain_key.extend_from_slice(key);
    let mut wrapped_key = encrypt_ecb(&plain_key, CORE_KEY)?;
    for byte in &mut wrapped_key {
        *byte ^= 0x64;
    }
    let wrapped_meta = if let Some(comment) = comment {
        crate::media::tags::encode_comment(comment)?
            .bytes()
            .map(|b| b ^ 0x63)
            .collect()
    } else {
        match options.metadata {
            None => Vec::new(),
            Some(value) => {
                if !value.is_object() {
                    return Err(Error::InvalidInput("NCM metadata must be an object"));
                }
                let text = serde_json::to_vec(value)?;
                if text.len() as u64 > MAX_FIELD {
                    return Err(Error::InvalidInput("NCM metadata exceeds size limit"));
                }
                let mut plain = b"music:".to_vec();
                plain.extend_from_slice(&text);
                let encrypted = encrypt_ecb(&plain, META_KEY)?;
                let mut wrapped = META_PREFIX.to_vec();
                wrapped.extend_from_slice(STANDARD.encode(encrypted).as_bytes());
                for byte in &mut wrapped {
                    *byte ^= 0x63;
                }
                wrapped
            }
        }
    };
    let mut header = MAGIC.to_vec();
    header.extend_from_slice(&[1, 0x69]);
    append_field(&mut header, &wrapped_key)?;
    append_field(&mut header, &wrapped_meta)?;
    let crc = crc32fast::hash(&header);
    output.write_all(&header)?;
    output.write_all(&crc.to_le_bytes())?;
    output.write_all(&[1])?;
    let cover_size = (options.cover.len() as u32).to_le_bytes();
    output.write_all(&cover_size)?;
    output.write_all(&cover_size)?;
    output.write_all(options.cover)?;
    let mut md5 = Md5::new();
    let mut sha = Sha256::new();
    let mut offset = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = match source.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            value => value?,
        };
        if count == 0 {
            break;
        }
        let bytes = &mut buffer[..count];
        md5.update(&*bytes);
        sha.update(&*bytes);
        cipher::apply(bytes, &mask, offset);
        output.write_all(bytes)?;
        offset = offset
            .checked_add(count as u64)
            .ok_or(Error::InvalidInput("NCM audio size overflow"))?;
    }
    if offset == 0 {
        return Err(Error::InvalidInput("empty NCM audio payload"));
    }
    output.flush()?;
    Ok(NcmWriteReport {
        audio_bytes: offset,
        audio_md5: hex::encode(md5.finalize()),
        audio_sha256: hex::encode(sha.finalize()),
        header_crc32: crc,
        cover_bytes: options.cover.len(),
    })
}

pub fn write_file(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: NcmWriteOptions<'_>,
) -> Result<NcmWriteReport> {
    let mut source = crate::media::open_regular(source)?;
    let mut temporary = export::staging(output.as_ref())?;
    let report = write_stream(&mut source, &mut temporary, options)?;
    export::publish(temporary, output.as_ref())?;
    Ok(report)
}
