//! Optional scan evidence for a whole WAV on a fixed local NTFS volume.

use crate::{
    Result,
    library::identity::{FileFingerprint, fingerprint_metadata, windows::WindowsFileIdentity},
};
use std::{fs::File, path::Path};

pub(super) fn witnessed(expected: &FileFingerprint) -> bool {
    expected
        .file_identity
        .as_deref()
        .is_some_and(|identity| identity.starts_with(WindowsFileIdentity::INDEXED_WAV_PREFIX))
}

fn held_fingerprint(file: &File, identity: WindowsFileIdentity) -> Result<FileFingerprint> {
    let mut fingerprint = fingerprint_metadata(file.metadata()?)?;
    fingerprint.file_identity = Some(identity.indexed_wav_marker());
    Ok(fingerprint)
}

pub(super) fn is_wave(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

pub(super) fn observe(path: &Path) -> Result<Option<FileFingerprint>> {
    if !is_wave(path) {
        return Ok(None);
    }
    let Some((mut file, identity)) = WindowsFileIdentity::open_local_fixed_ntfs_nofollow(path)?
    else {
        return Ok(None);
    };
    if !super::held_audio::IndexedAudioFormat::Wave.check_container(&mut file)? {
        return Ok(None);
    }
    Ok(Some(held_fingerprint(&file, identity)?))
}

pub(super) fn matches(file: &File, expected: &FileFingerprint) -> Result<bool> {
    let Some(identity) = WindowsFileIdentity::current_indexed_wav(file)? else {
        return Ok(false);
    };
    Ok(held_fingerprint(file, identity)? == *expected)
}
