//! Explicit files supplied by the native application entry, not a library scan.
use super::{
    AudioMetadata, LocalResource, LocalRevision,
    identity::{FileFingerprint, fingerprint_metadata, path_id},
    probe::AudioProbe,
};
use crate::{
    Error, Result,
    media::open_regular,
    playback::engine::{pcm_wave, pinned_audio_format},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub(crate) const MAX_NATIVE_FILES: usize = 8;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenedFile {
    pub path: PathBuf,
    pub fingerprint: FileFingerprint,
}

fn safe_path(path: &Path) -> bool {
    if !path.is_absolute()
        || path
            .to_str()
            .is_none_or(|value| value.chars().any(char::is_control))
    {
        return false;
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return false;
    }
    true
}

fn format_from_path(path: &Path) -> Result<&'static str> {
    let extension = path.extension();
    if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("flac")) {
        Ok("flac")
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("mp3")) {
        Ok("mp3")
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("wav")) {
        Ok("wav")
    } else {
        Err(Error::InvalidInput("unsupported native open audio format"))
    }
}

pub(crate) fn checked_audio_file(
    path: &Path,
    expected: &FileFingerprint,
) -> Result<(File, &'static str)> {
    if !safe_path(path) || path.canonicalize()? != path {
        return Err(Error::InvalidInput("invalid native local audio path"));
    }
    let format = format_from_path(path)?;
    let mut file = open_regular(path)?;
    if fingerprint_metadata(file.metadata()?)? != *expected
        || pinned_audio_format(&mut file)? != format
    {
        return Err(Error::State("native local audio changed"));
    }
    if format == "wav" {
        pcm_wave::PcmWave::inspect(&mut file)?;
        if fingerprint_metadata(file.metadata()?)? != *expected {
            return Err(Error::State("native local audio changed"));
        }
    }
    Ok((file, format))
}

pub(crate) fn inspect_opened_file(path: &Path, probe: &dyn AudioProbe) -> Result<Value> {
    if !safe_path(path) {
        return Err(Error::InvalidInput("invalid native local audio path"));
    }
    let format = format_from_path(path)?;
    let path = path.canonicalize()?;
    if !safe_path(&path) || format_from_path(&path)? != format {
        return Err(Error::InvalidInput(
            "native local audio target changed format",
        ));
    }
    let mut file = open_regular(&path)?;
    let fingerprint = fingerprint_metadata(file.metadata()?)?;
    let metadata = if format == "wav" {
        // The same native PCM layout defines the queue duration and decoder.
        // No path-based probe or alternate audio stream gets to pick either.
        let layout = pcm_wave::PcmWave::inspect(&mut file)?;
        if fingerprint_metadata(file.metadata()?)? != fingerprint {
            return Err(Error::State("native local audio changed"));
        }
        AudioMetadata {
            format: Some("pcm_s16le".into()),
            duration: layout.frames as f64 / f64::from(layout.rate),
            title: path
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .into(),
            album: String::new(),
            artists: Vec::new(),
            bitrate: Some(u64::from(layout.rate) * pcm_wave::BYTES_PER_FRAME as u64 * 8),
            sample_rate: Some(layout.rate),
            channels: Some(2),
            container: format.into(),
            cover_bytes: None,
        }
    } else {
        checked_audio_file(&path, &fingerprint)?;
        let metadata = probe.inspect(&path)?;
        if metadata.format.as_deref() != Some(format)
            || metadata.duration <= 0.0
            || !metadata.duration.is_finite()
        {
            return Err(Error::Media("invalid native local audio metadata"));
        }
        checked_audio_file(&path, &fingerprint)?;
        metadata
    };

    let local = LocalResource {
        local_id: path_id(&path)?,
        local_revision: LocalRevision::File {
            fingerprint: fingerprint.clone(),
        },
        metadata,
    };
    let mut resource = serde_json::to_value(local)?;
    resource["native_open_file"] = serde_json::to_value(OpenedFile { path, fingerprint })?;
    Ok(resource)
}

impl OpenedFile {
    pub(crate) fn check_resource(&self, resource: &LocalResource) -> Result<()> {
        if resource.local_id != path_id(&self.path)?
            || resource.local_revision
                != (LocalRevision::File {
                    fingerprint: self.fingerprint.clone(),
                })
        {
            return Err(Error::InvalidInput("native local audio identity changed"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::engine::pcm_wave::tests::{wave, wave_at};

    struct NoPathProbe;
    impl AudioProbe for NoPathProbe {
        fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
            panic!("PCM WAV must get metadata from its native file")
        }
    }

    #[test]
    fn native_wav_metadata_uses_the_selected_48k_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("48000.wav");
        std::fs::write(&path, wave_at(48_000, std::iter::repeat_n([0, 0], 24_000))).unwrap();
        let resource = inspect_opened_file(&path, &NoPathProbe).unwrap();
        let local: LocalResource = serde_json::from_value(resource).unwrap();
        assert_eq!(local.metadata.duration, 0.5);
        assert_eq!(local.metadata.sample_rate, Some(48_000));
        assert_eq!(local.metadata.bitrate, Some(1_536_000));
    }

    #[test]
    fn native_pcm_wav_receipt_round_trips_and_rejects_the_replaced_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("合成 波形.WAV");
        let bytes = wave(std::iter::repeat_n([256, -128], 22050));
        std::fs::write(&path, &bytes).unwrap();
        let resource = inspect_opened_file(&path, &NoPathProbe).unwrap();
        let local: LocalResource = serde_json::from_value(resource.clone()).unwrap();
        let opened: OpenedFile =
            serde_json::from_value(resource["native_open_file"].clone()).unwrap();
        opened.check_resource(&local).unwrap();
        assert_eq!(local.metadata.title, "合成 波形");
        assert_eq!(local.metadata.duration, 0.5);
        assert_eq!(local.metadata.format.as_deref(), Some("pcm_s16le"));
        assert_eq!(
            checked_audio_file(&opened.path, &opened.fingerprint)
                .unwrap()
                .1,
            "wav"
        );

        let mut incompatible = bytes;
        incompatible[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        std::fs::write(&path, incompatible).unwrap();
        let fingerprint = super::super::identity::fingerprint(&path).unwrap();
        assert!(checked_audio_file(&path, &fingerprint).is_err());
        assert!(inspect_opened_file(&path, &NoPathProbe).is_err());
    }

    #[test]
    fn pinned_file_refuses_changed_identity_and_extension_spoof() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let path = dir.join("合成 空格.flac");
        std::fs::write(&path, b"fLaCabcdefgh").unwrap();
        let expected = super::super::identity::fingerprint(&path).unwrap();
        assert_eq!(checked_audio_file(&path, &expected).unwrap().1, "flac");
        std::fs::write(&path, b"fLaC1234567890").unwrap();
        assert!(checked_audio_file(&path, &expected).is_err());
        std::fs::write(&path, b"ID3abcdefghij").unwrap();
        let current = super::super::identity::fingerprint(&path).unwrap();
        assert!(checked_audio_file(&path, &current).is_err());
    }
}
