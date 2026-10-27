//! FFprobe is an explicit cross-platform process adapter. No shell is used;
//! network protocols are disabled. Hosts supply/bundle the executable.
mod process;
mod response;
use super::AudioMetadata;
use crate::{
    Error, Result,
    media::ncm::{HeaderPolicy, NcmReader},
};
pub use response::parse_ffprobe_response;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub trait AudioProbe: Send + Sync {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata>;
}

pub struct Ffprobe {
    executable: PathBuf,
    timeout: Duration,
}
impl Default for Ffprobe {
    fn default() -> Self {
        // Packaged hosts may ship the tool beside their executable. Resolve
        // that trusted installation path explicitly; never depend on the CWD.
        let name = if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        };
        let executable = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join(name)))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(name));
        Self {
            executable,
            timeout: Duration::from_secs(30),
        }
    }
}
impl Ffprobe {
    pub fn new(executable: PathBuf, timeout: Duration) -> Result<Self> {
        if executable.as_os_str().is_empty()
            || timeout.is_zero()
            || timeout > Duration::from_secs(300)
        {
            return Err(Error::InvalidInput("invalid media probe configuration"));
        }
        Ok(Self {
            executable,
            timeout,
        })
    }
}
impl AudioProbe for Ffprobe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        let path = path.canonicalize()?;
        crate::media::open_regular(&path)?;
        if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("ncm"))
        {
            return inspect_ncm(&path);
        }
        let value = process::run(&self.executable, &path, self.timeout)?;
        parse_ffprobe_response(&path, &value)
    }
}

fn inspect_ncm(path: &Path) -> Result<AudioMetadata> {
    let reader = NcmReader::open(path, HeaderPolicy::Compatible)?;
    let info = reader.info();
    let data = info.metadata.as_ref().unwrap_or(&Value::Null);
    let duration = response::finite_number(&data["duration"], 0.0)? / 1000.0;
    let artists = data["artist"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().or_else(|| v[0].as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let format = match info.audio_format {
        Some(super::super::media::ncm::AudioFormat::Flac) => Some("flac".into()),
        Some(super::super::media::ncm::AudioFormat::Mp3) => Some("mp3".into()),
        Some(super::super::media::ncm::AudioFormat::M4a) => Some("m4a".into()),
        None => None,
    };
    Ok(AudioMetadata {
        format,
        duration,
        title: data["musicName"]
            .as_str()
            .unwrap_or_else(|| path.file_stem().and_then(|v| v.to_str()).unwrap_or(""))
            .into(),
        album: data["album"].as_str().unwrap_or("").into(),
        artists,
        bitrate: response::integer(&data["bitrate"])?,
        sample_rate: None,
        channels: None,
        container: "ncm".into(),
        cover_bytes: Some(info.cover_bytes),
    })
}
