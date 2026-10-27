use super::{HeaderPolicy, NcmInfo, NcmReader};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};
use tempfile::NamedTempFile;

#[derive(Debug, Serialize)]
pub struct ExtractReport {
    pub info: NcmInfo,
    pub audio_sha256: String,
}

pub(super) fn staging(output: &Path) -> Result<NamedTempFile> {
    match fs::symlink_metadata(output) {
        Ok(_) => {
            return Err(
                io::Error::new(io::ErrorKind::AlreadyExists, "output already exists").into(),
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    Ok(tempfile::Builder::new()
        .prefix(".cumulus-ncm-")
        .tempfile_in(parent)?)
}

pub(super) fn publish(mut temporary: NamedTempFile, output: &Path) -> Result<()> {
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    // No-clobber publication is checked again by the filesystem, even when a
    // destination appears after staging. On some platforms a temp link can
    // remain after a process crash; no directory-fsync guarantee is claimed.
    temporary
        .persist_noclobber(output)
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

pub fn extract(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    policy: HeaderPolicy,
) -> Result<ExtractReport> {
    let mut reader = NcmReader::open(source, policy)?;
    if reader.info().audio_format.is_none() {
        return Err(Error::Media("unrecognized NCM audio; export refused"));
    }
    let mut temporary = staging(output.as_ref())?;
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        temporary.write_all(&buffer[..count])?;
        sha.update(&buffer[..count]);
    }
    let report = ExtractReport {
        info: reader.info().clone(),
        audio_sha256: hex::encode(sha.finalize()),
    };
    publish(temporary, output.as_ref())?;
    Ok(report)
}
