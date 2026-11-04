use super::RemoteSkinImage;
use crate::{
    Error, Result,
    download::transfer::{MediaBackend, MediaRequest, MediaUrl},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use serde_json::Value;
use std::io::Cursor;

const MAX_SOURCE: usize = 5 * 1024 * 1024;
const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u64 = 12_000_000;
const MAX_PNG: usize = 3 * 1024 * 1024;

pub(super) fn url(value: &Value) -> Option<MediaUrl> {
    let raw = value.as_str()?;
    if raw.len() > 2048
        || raw.trim() != raw
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
    {
        return None;
    }
    let mut target = url::Url::parse(raw).ok()?;
    if !matches!(target.scheme(), "http" | "https")
        || !target.username().is_empty()
        || target.password().is_some()
        || target.port().is_some()
    {
        return None;
    }
    let shard = target
        .host_str()?
        .strip_suffix(".music.126.net")?
        .strip_prefix('p')?;
    if !(1..=2).contains(&shard.len()) || !shard.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut segments = target.path_segments()?.collect::<Vec<_>>();
    if segments.len() > 12
        || segments.iter().any(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return None;
    }
    let file = segments.pop()?;
    let (stem, ext) = file.rsplit_once('.')?;
    if stem.is_empty()
        || !matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp"
        )
    {
        return None;
    }
    let object = segments.first() == Some(&"obj") && segments.len() >= 2;
    let legacy = segments.len() == 1
        && (16..=32).contains(&segments[0].len())
        && stem.bytes().all(|b| b.is_ascii_digit());
    if !object && !legacy {
        return None;
    }
    target.set_scheme("https").ok()?;
    target.set_query(None);
    target.set_fragment(None);
    MediaUrl::parse(target.as_str()).ok()
}

pub(super) async fn download(backend: &dyn MediaBackend, url: MediaUrl) -> Result<Vec<u8>> {
    let reply = backend
        .get(MediaRequest {
            url,
            offset: 0,
            if_range: None,
        })
        .await?;
    if reply.status != 200
        || reply.headers.contains_key("content-range")
        || reply
            .headers
            .get("content-encoding")
            .is_some_and(|e| !e.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::Media("skin image response rejected"));
    }
    let length = reply
        .headers
        .get("content-length")
        .map(|length| {
            length
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_SOURCE).contains(n))
                .ok_or(Error::Media("skin image size exceeds limit"))
        })
        .transpose()?;
    let mut output = Vec::with_capacity(length.unwrap_or_default());
    let mut body = reply.body;
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = body.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let chunk = buffer
            .get(..count)
            .ok_or(Error::Media("invalid skin image chunk"))?;
        if chunk.len() > MAX_SOURCE.saturating_sub(output.len()) {
            return Err(Error::Media("skin image size exceeds limit"));
        }
        output.extend_from_slice(chunk);
    }
    if output.is_empty() || length.is_some_and(|n| n != output.len()) {
        return Err(Error::Media("skin image body is incomplete"));
    }
    Ok(output)
}

pub(super) fn render(bytes: &[u8], kind: RemoteSkinImage) -> Result<String> {
    let format = image::guess_format(bytes).map_err(|_| Error::Media("invalid skin image"))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Bmp
    ) {
        return Err(Error::Media("unsupported skin image"));
    }
    let reader = || ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader()
        .into_dimensions()
        .map_err(|_| Error::Media("invalid skin image dimensions"))?;
    if !(1..=MAX_SIDE).contains(&width)
        || !(1..=MAX_SIDE).contains(&height)
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Media("skin image dimensions exceed limit"));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = reader();
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| Error::Media("invalid skin image data"))?;
    let (w, h) = match kind {
        RemoteSkinImage::Cover => (320, 220),
        RemoteSkinImage::Preview => (1024, 720),
    };
    let image = if width > w || height > h {
        image.thumbnail(w, h)
    } else {
        image
    };
    let mut png = Cursor::new(Vec::new());
    image
        .into_rgba8()
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|_| Error::Media("cannot encode skin image"))?;
    if png.get_ref().len() > MAX_PNG {
        return Err(Error::Media("rendered skin image exceeds limit"));
    }
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png.into_inner())
    ))
}
