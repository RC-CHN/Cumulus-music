//! Nonblocking process locks using the portable standard-library file API.
//! Keep the handle across async I/O; never hold an account/SQLite mutex there.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, TryLockError},
    path::Path,
};

pub struct OperationLock {
    _file: File,
}
impl OperationLock {
    pub fn acquire(directory: &Path, namespace: &str, key: &str) -> Result<Self> {
        let root = directory.join("operation-locks");
        let builder = crate::storage::private_directory_builder();
        match builder.create(&root) {
            Ok(()) => (),
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&root)?.is_dir() => {}
            Err(error) => return Err(error.into()),
        }
        let key = serde_json::to_vec(&(namespace, key))?;
        let path = root.join(format!("{}.lock", hex::encode(Sha256::digest(key))));
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && !metadata.is_file()
        {
            return Err(Error::State("operation lock is not a regular file"));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::MutationBusy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}
