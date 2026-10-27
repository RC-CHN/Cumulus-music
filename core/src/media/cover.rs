use crate::{Error, Result};
use image::{ImageFormat, ImageReader, Limits};
use serde::Serialize;
use std::io::Cursor;

#[derive(Clone, Debug, Serialize)]
pub struct CoverInfo {
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Decode bounded PNG/JPEG covers before writing media metadata. The original
/// bytes are retained by the caller; validation does not transcode artwork.
pub fn validate_cover(data: &[u8]) -> Result<CoverInfo> {
    if data.is_empty() || data.len() > 4 * 1024 * 1024 {
        return Err(Error::InvalidInput("cover must contain 1 to 4MiB bytes"));
    }
    let format =
        image::guess_format(data).map_err(|_| Error::Media("unrecognized cover format"))?;
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(Error::Media("cover must be PNG or JPEG")),
    };
    if format == ImageFormat::Png {
        validate_png_chunks(data)?;
    }
    if format == ImageFormat::Jpeg && !data.ends_with(&[0xff, 0xd9]) {
        return Err(Error::Media("truncated JPEG cover"));
    }
    let (width, height) = ImageReader::with_format(Cursor::new(data), format)
        .into_dimensions()
        .map_err(|_| Error::Media("invalid cover dimensions"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(Error::Media("cover dimensions exceed limit"));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| Error::Media("invalid cover image"))?;
    Ok(CoverInfo {
        mime,
        width,
        height,
    })
}

fn validate_png_chunks(data: &[u8]) -> Result<()> {
    let mut offset = 8usize;
    while offset < data.len() {
        let header = data
            .get(offset..offset + 8)
            .ok_or(Error::Media("truncated PNG chunk"))?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four bytes")) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= data.len())
            .ok_or(Error::Media("PNG chunk exceeds cover bounds"))?;
        let checksum = u32::from_be_bytes(data[end - 4..end].try_into().expect("four bytes"));
        if crc32fast::hash(&data[offset + 4..end - 4]) != checksum {
            return Err(Error::Media("PNG chunk CRC mismatch"));
        }
        if &header[4..8] == b"IEND" {
            if length != 0 || end != data.len() {
                return Err(Error::Media("invalid PNG end marker"));
            }
            return Ok(());
        }
        offset = end;
    }
    Err(Error::Media("missing PNG end marker"))
}
