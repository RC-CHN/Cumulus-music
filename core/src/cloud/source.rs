//! Immutable input facts shared by cloud audio and attachment transfers.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Source {
    pub path: PathBuf,
    pub bytes: u64,
    pub md5: String,
    pub sha256: String,
}
pub(super) type Reader = Arc<Mutex<File>>;
impl Source {
    pub fn validate(&self) -> Result<()> {
        super::api::validate_digest(&self.md5)?;
        crate::media::integrity::FileIntegrity {
            bytes: self.bytes,
            sha256: self.sha256.clone(),
        }
        .validate()?;
        if !self.path.is_absolute() || self.bytes > 512 * 1024 * 1024 {
            return Err(Error::State("invalid persisted upload source"));
        }
        Ok(())
    }
    pub fn inspect(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let mut file = crate::media::open_regular(&path)?;
        let (bytes, md5, sha256) = digest(&mut file)?;
        Ok(Self {
            path,
            bytes,
            md5,
            sha256,
        })
    }
    pub fn open(&self) -> Result<Reader> {
        let mut file = crate::media::open_regular(&self.path)?;
        self.verify(&mut file)?;
        Ok(Arc::new(Mutex::new(file)))
    }
    pub fn verify(&self, file: &mut File) -> Result<()> {
        let (bytes, md5, sha256) = digest(file)?;
        if bytes != self.bytes || md5 != self.md5 || sha256 != self.sha256 {
            return Err(Error::Media("upload source changed"));
        }
        Ok(())
    }
    pub async fn open_async(&self) -> Result<Reader> {
        let input = self.clone();
        tokio::task::spawn_blocking(move || input.open())
            .await
            .map_err(|_| Error::State("upload source worker stopped"))?
    }
    pub async fn verify_async(&self, reader: Reader) -> Result<()> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || {
            source.verify(&mut *reader.lock().map_err(|_| Error::LockPoisoned)?)?;
            // Also reject replacing the named path while the original handle
            // remains readable; publication must describe this same input.
            if Self::inspect(&source.path)? != source {
                return Err(Error::Media("upload source path changed"));
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::State("upload source worker stopped"))?
    }
}
fn digest(file: &mut File) -> Result<(u64, String, String)> {
    let before = file.metadata()?;
    if !before.is_file() || before.len() == 0 || before.len() > 512 * 1024 * 1024 {
        return Err(Error::InvalidInput(
            "upload requires nonempty regular input up to 512 MiB",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut md5 = md5::Md5::new();
    let mut sha = sha2::Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0; 65536];
    loop {
        let n = match file.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            break;
        }
        if bytes + n as u64 > before.len() {
            return Err(Error::Media("upload input grew while hashing"));
        }
        md5.update(&buffer[..n]);
        sha.update(&buffer[..n]);
        bytes += n as u64;
    }
    let after = file.metadata()?;
    if bytes != before.len()
        || after.len() != before.len()
        || after.modified()? != before.modified()?
    {
        return Err(Error::Media("upload input changed while hashing"));
    }
    Ok((
        bytes,
        hex::encode(md5.finalize()),
        hex::encode(sha.finalize()),
    ))
}
pub(super) async fn chunk(reader: Reader, offset: u64, length: usize) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let mut file = reader.lock().map_err(|_| Error::LockPoisoned)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    })
    .await
    .map_err(|_| Error::State("upload chunk worker stopped"))?
}
