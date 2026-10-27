use super::{TransferIntegrity, TransferJob};
use crate::{Error, Result};
use md5::Md5;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek},
    path::{Path, PathBuf},
};

pub(super) use crate::download::file_identity::{Identity as FileToken, identity as token};
pub(super) fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "transfer output exists",
        )
        .into()),
    }
}
pub(super) fn target_path(target: &Path) -> Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or(Error::InvalidInput("transfer target requires a filename"))?;
    let absolute = std::path::absolute(target)?;
    let parent = absolute
        .parent()
        .ok_or(Error::InvalidInput("transfer target requires a parent"))?;
    fs::create_dir_all(parent)?;
    let path = parent.canonicalize()?.join(name);
    absent(&path)?;
    Ok(path)
}
pub(super) fn create_part(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
pub(super) fn check_parent(job: &TransferJob) -> Result<()> {
    let parent = job
        .target
        .parent()
        .ok_or(Error::State("missing transfer parent"))?;
    if !fs::symlink_metadata(parent)?.is_dir() || token(parent)? != job.parent_token {
        return Err(Error::State("transfer parent directory changed"));
    }
    if job.part != parent.join(format!(".cumulus-{}.part", job.id)) {
        return Err(Error::State("invalid stored transfer staging path"));
    }
    Ok(())
}
pub(super) fn owned_part(job: &TransferJob) -> Result<()> {
    check_parent(job)?;
    if !fs::symlink_metadata(&job.part)?.is_file() || token(&job.part)? != job.part_token {
        return Err(Error::State("transfer staging file is no longer owned"));
    }
    Ok(())
}
pub(super) fn open_part(job: &TransferJob) -> Result<File> {
    owned_part(job)?;
    let file = OpenOptions::new().read(true).write(true).open(&job.part)?;
    if !file.metadata()?.is_file()
        || same_file::Handle::from_file(file.try_clone()?)?
            != same_file::Handle::from_path(&job.part)?
    {
        return Err(Error::State("transfer staging file changed while opening"));
    }
    owned_part(job)?;
    Ok(file)
}

pub(super) fn matches_handle(file: &File, path: &Path) -> Result<bool> {
    Ok(fs::symlink_metadata(path)?.is_file()
        && same_file::Handle::from_file(file.try_clone()?)? == same_file::Handle::from_path(path)?)
}
pub(super) fn target_is_owned(job: &TransferJob) -> Result<bool> {
    check_parent(job)?;
    match fs::symlink_metadata(&job.target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(meta) => Ok(meta.is_file() && token(&job.target)? == job.part_token),
    }
}
pub(super) fn ensure_unpublished(job: &TransferJob) -> Result<()> {
    if target_is_owned(job)? {
        return Err(Error::State(
            "published transfer must be reconciled before mutation",
        ));
    }
    Ok(())
}
pub(super) fn remove_part(job: &TransferJob) -> Result<()> {
    check_parent(job)?;
    match fs::symlink_metadata(&job.part) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
        Ok(_) => (),
    }
    owned_part(job)?;
    fs::remove_file(&job.part)?;
    sync_parent(job)
}
pub(super) fn sync_parent(job: &TransferJob) -> Result<()> {
    check_parent(job)?;
    #[cfg(unix)]
    {
        File::open(job.target.parent().expect("checked"))?.sync_all()?;
    }
    // Standard Rust cannot fsync a Windows directory. File sync/no-clobber
    // still apply there; directory crash durability is not claimed.
    Ok(())
}
pub(super) fn hash(mut file: File, limit: u64) -> Result<TransferIntegrity> {
    let before = file.metadata()?;
    if !before.is_file() || before.len() == 0 || before.len() > limit {
        return Err(Error::Media("invalid final transfer size"));
    }
    file.rewind()?;
    let mut buffer = [0u8; 65536];
    let mut bytes = 0u64;
    let mut md5 = Md5::new();
    let mut sha = Sha256::new();
    loop {
        let n = match file.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            value => value?,
        };
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > limit {
            return Err(Error::Media("transfer exceeds size limit"));
        }
        md5.update(&buffer[..n]);
        sha.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    if bytes != before.len()
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(Error::State("transfer changed during hashing"));
    }
    Ok(TransferIntegrity {
        bytes,
        md5: hex::encode(md5.finalize()),
        sha256: hex::encode(sha.finalize()),
    })
}
