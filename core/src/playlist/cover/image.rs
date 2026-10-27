use super::*;
use ::image::{
    DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, codecs::jpeg::JpegEncoder,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub(super) const INPUT_LIMIT: usize = 5 * 1024 * 1024;
pub(super) const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// Integer pixels in the oriented original, matching onCropComplete's second
/// argument. 200x200 is the official viewport, not an output resize request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverCrop {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}
impl CoverCrop {
    pub fn centered(width: u32, height: u32) -> Self {
        let size = width.min(height);
        // JS Math.round for nonnegative values, including odd margins.
        Self {
            x: (width - size).div_ceil(2),
            y: (height - size).div_ceil(2),
            size,
        }
    }
    pub(super) fn validate(self, width: u32, height: u32) -> Result<()> {
        if self.size == 0
            || self.x.checked_add(self.size).is_none_or(|v| v > width)
            || self.y.checked_add(self.size).is_none_or(|v| v > height)
        {
            return Err(Error::InvalidInput("cover crop is outside the image"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub crop: CoverCrop,
    /// Bounded, re-encoded JPEG; no local path, EXIF, or original file bytes.
    pub preview: String,
}
fn decode(data: &[u8]) -> Result<DynamicImage> {
    // M546: JPEG/PNG/BMP/GIF, at most 5 MiB, both dimensions at least 320.
    if data.is_empty() || data.len() > INPUT_LIMIT {
        return Err(Error::InvalidInput(
            "cover input must contain 1 to 5MiB bytes",
        ));
    }
    let format = ::image::guess_format(data).map_err(|_| Error::Media("invalid cover format"))?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Gif
    ) {
        return Err(Error::Media("cover must be JPEG, PNG, BMP or GIF"));
    }
    let mut reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| Error::Media("invalid cover format"))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| Error::Media("invalid cover image"))?;
    let (width, height) = decoder.dimensions();
    if width < 320 || height < 320 {
        return Err(Error::InvalidInput(
            "cover resolution must be at least 320x320",
        ));
    }
    if u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(Error::Media("cover dimensions exceed 16 megapixels"));
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| Error::Media("invalid image orientation"))?;
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|_| Error::Media("invalid cover image"))?;
    image.apply_orientation(orientation);
    Ok(image)
}
fn jpeg(image: DynamicImage) -> Result<Vec<u8>> {
    let mut rgba = image.to_rgba8();
    // Canvas toBlob(image/jpeg) composites transparent pixels onto black.
    for pixel in rgba.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
        pixel[3] = 255;
    }
    let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut bytes = Vec::new();
    // Native encoding uses explicit quality. Browser toBlob leaves quality to
    // the host; no cross-encoder JPEG bit identity is claimed.
    JpegEncoder::new_with_quality(&mut bytes, 92)
        .encode_image(&rgb)
        .map_err(|_| Error::Media("cover JPEG encoding failed"))?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err(Error::Media("cropped cover exceeds 16MiB"));
    }
    Ok(bytes)
}
pub fn inspect_image(data: &[u8]) -> Result<CoverImage> {
    let image = decode(data)?;
    let (width, height) = (image.width(), image.height());
    let preview = jpeg(image.thumbnail(1024, 1024))?;
    Ok(CoverImage {
        width,
        height,
        crop: CoverCrop::centered(width, height),
        preview: format!("data:image/jpeg;base64,{}", STANDARD.encode(preview)),
    })
}
pub(super) fn crop(data: &[u8], crop: CoverCrop) -> Result<Vec<u8>> {
    let image = decode(data)?;
    crop.validate(image.width(), image.height())?;
    jpeg(image.crop_imm(crop.x, crop.y, crop.size, crop.size))
}
