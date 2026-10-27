use super::super::AudioMetadata;
use crate::{Error, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub(super) fn finite_number(value: &Value, default: f64) -> Result<f64> {
    if value.is_null() {
        return Ok(default);
    }
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or(Error::Media("invalid finite audio duration"))
}
pub(super) fn integer(value: &Value) -> Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .map(Some)
        .ok_or(Error::Media("invalid audio numeric field"))
}
fn small(value: &Value) -> Result<Option<u32>> {
    integer(value)?
        .map(|n| u32::try_from(n).map_err(|_| Error::Media("audio numeric field out of range")))
        .transpose()
}

pub fn parse_ffprobe_response(path: &Path, value: &Value) -> Result<AudioMetadata> {
    let stream = value["streams"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["codec_type"] == "audio"))
        .ok_or(Error::Media("file has no audio stream"))?;
    let format = &value["format"];
    let tags: BTreeMap<_, _> = format["tags"]
        .as_object()
        .map(|values| {
            values
                .iter()
                .map(|(key, value)| (key.to_ascii_lowercase(), value))
                .collect()
        })
        .unwrap_or_default();
    let tag = |key: &str| tags.get(key).and_then(|v| v.as_str());
    Ok(AudioMetadata {
        format: stream["codec_name"].as_str().map(str::to_owned),
        duration: finite_number(&format["duration"], 0.0)?,
        title: tag("title")
            .unwrap_or_else(|| path.file_stem().and_then(|v| v.to_str()).unwrap_or(""))
            .into(),
        album: tag("album").unwrap_or("").into(),
        artists: tag("artist")
            .filter(|v| !v.is_empty())
            .map(|v| vec![v.into()])
            .unwrap_or_default(),
        bitrate: integer(&format["bit_rate"])?,
        sample_rate: small(&stream["sample_rate"])?,
        channels: small(&stream["channels"])?,
        container: path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase(),
        cover_bytes: None,
    })
}
