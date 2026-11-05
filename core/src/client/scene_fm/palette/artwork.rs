use crate::{
    Error, Result,
    download::transfer::{MediaBackend, MediaRequest, MediaUrl},
};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits, metadata::Orientation};
use std::io::Cursor;

pub(super) const MAX_BYTES: usize = 3 * 1024 * 1024;
pub(super) struct Image {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}
pub(super) fn thumbnail_size(ratio: f64) -> Result<u32> {
    if !ratio.is_finite() || !(0.5..=4.0).contains(&ratio) {
        return Err(Error::InvalidInput("invalid artwork pixel ratio"));
    }
    Ok((300.0 * ratio).round() as u32)
}
pub(super) fn source(raw: &str) -> Result<String> {
    if raw.len() > 2048
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
        || raw.trim() != raw
    {
        return Err(Error::InvalidInput("invalid scene FM artwork"));
    }
    let canonical = crate::protocol::artwork::original(&serde_json::json!(raw))
        .ok_or(Error::InvalidInput("scene FM artwork unavailable"))?;
    let url =
        url::Url::parse(&canonical).map_err(|_| Error::InvalidInput("invalid scene FM artwork"))?;
    let shard = url
        .host_str()
        .and_then(|h| h.strip_suffix(".music.126.net"))
        .and_then(|h| h.strip_prefix('p'))
        .filter(|s| (1..=2).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()));
    if shard.is_none()
        || url.path().contains("iadmusicmat")
        || url.path().len() > 1800
        || url.path().contains("..")
        || url.path().ends_with('/')
    {
        return Err(Error::InvalidInput("unsupported scene FM artwork"));
    }
    MediaUrl::parse(&canonical)?;
    Ok(canonical)
}
fn target(source: &str, size: u32, header: bool) -> Result<MediaUrl> {
    let mut url =
        url::Url::parse(source).map_err(|_| Error::InvalidInput("invalid scene FM artwork"))?;
    let query = if header {
        format!("xnos=1&imageView=&thumbnail={size}y{size}")
    } else {
        format!("imageView=&thumbnail={size}y{size}&type=webp&rotate=0&tostatic=0")
    };
    url.set_query(Some(&query));
    MediaUrl::parse(url.as_str())
}
pub(super) async fn header(
    backend: &dyn MediaBackend,
    source: &str,
    size: u32,
) -> Result<Option<String>> {
    let reply = backend
        .get(MediaRequest {
            url: target(source, size, true)?,
            offset: 0,
            if_range: None,
        })
        .await?;
    if reply.status != 200 || reply.headers.contains_key("content-range") {
        return Err(Error::Media("artwork metadata response rejected"));
    }
    // Dropping the reply cancels its body; a metadata hit never downloads it.
    Ok(reply.headers.get("x-nos-meta-n-m-cl").map(|value| {
        if value.len() > 4096 {
            "V:bounded".into()
        } else {
            value.clone()
        }
    }))
}
pub(super) async fn image(backend: &dyn MediaBackend, source: &str, size: u32) -> Result<Image> {
    let reply = backend
        .get(MediaRequest {
            url: target(source, size, false)?,
            offset: 0,
            if_range: None,
        })
        .await?;
    if reply.status != 200
        || reply.headers.contains_key("content-range")
        || reply
            .headers
            .get("content-encoding")
            .is_some_and(|s| !s.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::Media("artwork image response rejected"));
    }
    let length = reply
        .headers
        .get("content-length")
        .map(|s| {
            s.parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_BYTES).contains(n))
                .ok_or(Error::Media("artwork image size exceeds limit"))
        })
        .transpose()?;
    let mime = reply.headers.get("content-type").map(|s| {
        s.split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    });
    let mut bytes = Vec::with_capacity(length.unwrap_or_default());
    let mut body = reply.body;
    let mut buffer = [0; 16 * 1024];
    loop {
        let n = body.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let block = buffer
            .get(..n)
            .ok_or(Error::Media("invalid artwork image chunk"))?;
        if block.len() > MAX_BYTES.saturating_sub(bytes.len()) {
            return Err(Error::Media("artwork image size exceeds limit"));
        }
        bytes.extend_from_slice(block);
    }
    if bytes.is_empty() || length.is_some_and(|n| n != bytes.len()) {
        return Err(Error::Media("artwork image body incomplete"));
    }
    inspect(bytes, mime.as_deref())
}
pub(super) fn inspect(bytes: Vec<u8>, advertised: Option<&str>) -> Result<Image> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Media("artwork image size exceeds limit"));
    }
    let format = image::guess_format(&bytes).map_err(|_| Error::Media("invalid artwork image"))?;
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::WebP => "image/webp",
        _ => return Err(Error::Media("unsupported artwork image")),
    };
    if advertised.is_some_and(|m| m != mime && !(mime == "image/jpeg" && m == "image/jpg")) {
        return Err(Error::Media("artwork image type mismatch"));
    }
    let (width, height) = if format == ImageFormat::WebP {
        webp_dimensions(&bytes)?
    } else {
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let mut decoder = reader
            .into_decoder()
            .map_err(|_| Error::Media("invalid artwork dimensions"))?;
        let (width, height) = decoder.dimensions();
        let orientation = decoder
            .orientation()
            .map_err(|_| Error::Media("invalid artwork orientation"))?;
        match orientation {
            Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH => (height, width),
            _ => (width, height),
        }
    };
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 12_000_000
    {
        return Err(Error::Media("artwork dimensions exceed limit"));
    }
    let sample_width = width.min(50);
    let sample_height = (u64::from(height) * u64::from(sample_width)).div_ceil(u64::from(width));
    if u64::from(sample_width) * sample_height > 16_384 {
        return Err(Error::Media("artwork sample dimensions exceed limit"));
    }
    Ok(Image {
        bytes,
        mime,
        width,
        height,
    })
}
fn webp_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    fn number(bytes: &[u8]) -> u32 {
        bytes
            .iter()
            .enumerate()
            .fold(0, |n, (i, b)| n | (u32::from(*b) << (i * 8)))
    }
    let invalid = || Error::Media("invalid WebP artwork dimensions");
    if bytes.get(..4) != Some(b"RIFF")
        || bytes.get(8..12) != Some(b"WEBP")
        || bytes
            .get(4..8)
            .is_none_or(|b| number(b) as usize != bytes.len().saturating_sub(8))
    {
        return Err(invalid());
    }
    let mut at = 12usize;
    for _ in 0..64 {
        let kind = bytes.get(at..at + 4).ok_or_else(invalid)?;
        let length = bytes.get(at + 4..at + 8).map(number).ok_or_else(invalid)? as usize;
        let payload = bytes.get(at + 8..at + 8 + length).ok_or_else(invalid)?;
        match kind {
            b"VP8X" if payload.len() >= 10 => {
                return Ok((number(&payload[4..7]) + 1, number(&payload[7..10]) + 1));
            }
            b"VP8L" if payload.len() >= 5 && payload[0] == 0x2f => {
                let bits = number(&payload[1..5]);
                return Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            b"VP8 " if payload.len() >= 10 && &payload[3..6] == b"\x9d\x01\x2a" => {
                return Ok((
                    number(&payload[6..8]) & 0x3fff,
                    number(&payload[8..10]) & 0x3fff,
                ));
            }
            _ => {}
        }
        at = at
            .checked_add(8 + length + (length & 1))
            .ok_or_else(invalid)?;
    }
    Err(invalid())
}
