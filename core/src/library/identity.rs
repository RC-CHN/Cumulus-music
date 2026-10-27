use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub bytes: u64,
    /// Decimal nanoseconds avoid JS number precision loss and retain pre-epoch dates.
    pub modified_ns: String,
    pub file_identity: Option<String>,
}

fn timestamp(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos().to_string(),
        Err(error) => format!("-{}", error.duration().as_nanos()),
    }
}

pub fn fingerprint(path: &Path) -> Result<FileFingerprint> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput("local source must be a regular file"));
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    };
    #[cfg(not(unix))]
    let identity = metadata
        .created()
        .ok()
        .map(|value| format!("created:{}", timestamp(value)));
    Ok(FileFingerprint {
        bytes: metadata.len(),
        modified_ns: timestamp(metadata.modified()?),
        file_identity: identity,
    })
}

pub fn file_id(path: &Path) -> Result<String> {
    let path = path.canonicalize()?;
    path_id(&path)
}
pub(super) fn path_id(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or(Error::InvalidInput("library path requires lossless UTF-8"))?;
    Ok(hex::encode(Sha256::digest(value.as_bytes())))
}
pub fn cue_track_id(path: &Path, track: u8) -> Result<String> {
    let path = path.canonicalize()?;
    let path = path
        .to_str()
        .ok_or(Error::InvalidInput("CUE path requires lossless UTF-8"))?;
    Ok(hex::encode(Sha256::digest(
        format!("cue\0{path}\0{track}").as_bytes(),
    )))
}
