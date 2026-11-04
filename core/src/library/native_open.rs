//! Explicit files supplied by the native application entry, not a library scan.
use super::{
    LocalResource, LocalRevision,
    identity::{FileFingerprint, fingerprint_metadata, path_id},
    probe::AudioProbe,
};
use crate::{Error, Result, media::open_regular, playback::engine::pinned_audio_format};
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
    let file = open_regular(&path)?;
    let fingerprint = fingerprint_metadata(file.metadata()?)?;
    checked_audio_file(&path, &fingerprint)?;
    let metadata = probe.inspect(&path)?;
    if metadata.format.as_deref() != Some(format)
        || metadata.duration <= 0.0
        || !metadata.duration.is_finite()
    {
        return Err(Error::Media("invalid native local audio metadata"));
    }
    checked_audio_file(&path, &fingerprint)?;

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
