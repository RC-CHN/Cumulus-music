//! Streaming file integrity shared by offline records and transfer pipelines.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIntegrity {
    pub bytes: u64,
    pub sha256: String,
}
impl FileIntegrity {
    pub fn validate(&self) -> Result<()> {
        if self.bytes == 0
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::InvalidInput(
                "expected a nonempty file and lowercase SHA256",
            ));
        }
        Ok(())
    }
}

pub fn inspect_file(path: impl AsRef<Path>) -> Result<FileIntegrity> {
    inspect_file_until(path, || false)
}
pub fn inspect_file_until(
    path: impl AsRef<Path>,
    cancel: impl Fn() -> bool,
) -> Result<FileIntegrity> {
    let path = path.as_ref();
    let before = fs::metadata(path)?;
    if !before.is_file() {
        return Err(Error::InvalidInput(
            "integrity source must be a regular file",
        ));
    }
    let mut file = super::open_regular(path)?;
    if !same(&before, &file.metadata()?)? {
        return Err(Error::State("file changed before integrity check"));
    }
    let mut sha = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        if cancel() {
            return Err(Error::Cancelled);
        }
        let n = match file.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            value => value?,
        };
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
        bytes = bytes
            .checked_add(n as u64)
            .ok_or(Error::Media("file size overflow"))?;
    }
    if bytes != before.len()
        || !same(&before, &file.metadata()?)?
        || !same(&before, &fs::metadata(path)?)?
    {
        return Err(Error::State("file changed during integrity check"));
    }
    Ok(FileIntegrity {
        bytes,
        sha256: hex::encode(sha.finalize()),
    })
}
fn same(a: &fs::Metadata, b: &fs::Metadata) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return Ok(false);
        }
    }
    Ok(a.is_file() && b.is_file() && a.len() == b.len() && a.modified()? == b.modified()?)
}
