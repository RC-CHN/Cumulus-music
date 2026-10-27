use super::{
    LIMIT,
    records::{Record, VerifiedMaterial},
};
use crate::{Error, Result, download::transfer::TransferJob, effects::ncae};
use md5::Md5;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn root(store: &Path, owner: &str, record: &Record) -> Result<PathBuf> {
    let mut path = store.canonicalize()?;
    for segment in [
        "effect-materials",
        &hex::encode(Sha256::digest(owner.as_bytes())),
        &record.handle.key,
    ] {
        path.push(segment);
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => (),
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&path)?.is_dir() => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}
pub(super) fn target(root: &Path, attempt: &str) -> PathBuf {
    root.join(format!("{attempt}.ncae"))
}
pub(super) fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn verify(job: &TransferJob) -> Result<VerifiedMaterial> {
    let expected = job
        .integrity
        .as_ref()
        .ok_or(Error::State("material transfer has no verified digest"))?;
    if !fs::symlink_metadata(&job.target)?.is_file() {
        return Err(Error::State("material is not a regular file"));
    }
    let mut file = crate::media::open_regular(&job.target)?;
    let before = file.metadata()?;
    if before.len() != expected.bytes || before.len() > LIMIT {
        return Err(Error::Media("material file size changed"));
    }
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != expected.bytes
        || hex::encode(Md5::digest(&bytes)) != expected.md5
        || hex::encode(Sha256::digest(&bytes)) != expected.sha256
        || file.metadata()?.modified()? != before.modified()?
        || !fs::symlink_metadata(&job.target)?.is_file()
        || same_file::Handle::from_file(file)? != same_file::Handle::from_path(&job.target)?
    {
        return Err(Error::Media("material changed after transfer verification"));
    }
    let container = ncae::decode(
        &bytes,
        ncae::Limits {
            container_bytes: LIMIT as usize,
            plain_bytes: 4 * 1024 * 1024,
        },
    )?;
    let parameters: serde_json::Value = serde_json::from_slice(&container.plain)?;
    if !parameters.is_object() {
        return Err(Error::Media("effect parameter material is not an object"));
    }
    Ok(VerifiedMaterial {
        path: job.target.clone(),
        parameters,
        type_id: container.type_id,
        plain_sha256: hex::encode(Sha256::digest(&container.plain)),
    })
}
