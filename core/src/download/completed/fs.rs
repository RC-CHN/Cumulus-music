use crate::{
    Error, Result,
    download::file_identity::{Identity, identity},
    library::identity::{FileFingerprint, fingerprint},
    media::integrity::{FileIntegrity, inspect_file},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Facts {
    pub identity: Identity,
    pub integrity: FileIntegrity,
    pub fingerprint: FileFingerprint,
    pub modified: SystemTime,
    pub readonly: bool,
    pub mode: Option<u32>,
}
pub(crate) fn facts(path: &Path) -> Result<Facts> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(Error::State("managed output is not a regular file"));
    }
    let identity = identity(path)?;
    let before = fingerprint(path)?;
    let integrity = inspect_file(path)?;
    if before != fingerprint(path)? || identity != crate::download::file_identity::identity(path)? {
        return Err(Error::State("managed output changed while reading"));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode = None;
    Ok(Facts {
        identity,
        integrity,
        fingerprint: before,
        modified: meta.modified()?,
        readonly: meta.permissions().readonly(),
        mode,
    })
}
/// A cheap selection-time pin using an already recorded content digest. This
/// does not claim the current bytes match; destructive workers still call facts.
pub(crate) fn recorded_facts(path: &Path, integrity: FileIntegrity) -> Result<Facts> {
    integrity.validate()?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() != integrity.bytes {
        return Err(Error::State("recorded output is missing or changed"));
    }
    let identity = identity(path)?;
    let before = fingerprint(path)?;
    if before != fingerprint(path)? || identity != crate::download::file_identity::identity(path)? {
        return Err(Error::State("recorded output changed while pinning"));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode = None;
    Ok(Facts {
        identity,
        integrity,
        fingerprint: before,
        modified: meta.modified()?,
        readonly: meta.permissions().readonly(),
        mode,
    })
}
pub(crate) fn parent(path: &Path, expected: &Identity) -> Result<()> {
    let p = path
        .parent()
        .ok_or(Error::State("managed file lacks a parent"))?;
    if p.canonicalize()? != p || !fs::symlink_metadata(p)?.is_dir() || identity(p)? != *expected {
        return Err(Error::State("managed parent directory was replaced"));
    }
    Ok(())
}
pub(crate) fn matches(path: &Path, expected: &Facts) -> Result<()> {
    let actual = facts(path)?;
    if actual.identity != expected.identity
        || actual.integrity != expected.integrity
        || actual.fingerprint != expected.fingerprint
    {
        return Err(Error::State(
            "managed file no longer matches recorded output",
        ));
    }
    Ok(())
}
/// Short recheck after a full hash, suitable for the final account-guarded step.
pub(crate) fn unchanged(path: &Path, expected: &Facts) -> Result<()> {
    if !fs::symlink_metadata(path)?.is_file()
        || identity(path)? != expected.identity
        || fingerprint(path)? != expected.fingerprint
    {
        return Err(Error::State(
            "managed file changed after integrity verification",
        ));
    }
    Ok(())
}
pub(crate) fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(Error::State("file destination already exists")),
    }
}
pub(crate) fn normalize_target(path: &Path, state: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let name = path
        .file_name()
        .ok_or(Error::InvalidInput("destination needs a file name"))?;
    let p = path
        .parent()
        .ok_or(Error::InvalidInput("destination needs a parent"))?;
    fs::create_dir_all(p)?;
    let target = p.canonicalize()?.join(name);
    if target.to_str().is_none() || target.starts_with(state.canonicalize()?) {
        return Err(Error::InvalidInput(
            "destination must be UTF-8 and outside private state",
        ));
    }
    Ok(target)
}
pub(crate) fn sync(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub(crate) fn path_key(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(
        path.to_str()
            .ok_or(Error::InvalidInput("file path must be UTF-8"))?
            .as_bytes(),
    )))
}
