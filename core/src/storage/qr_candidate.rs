//! Native lease for fresh ordinary QR candidate stores. A retired marker is
//! intentionally retained: opening the path again must not recreate the DB.
use crate::{Error, Result};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::Path,
};

const MARKER: &str = ".cumulus-qr-candidate-v1";
const DATABASE_FILES: [&str; 4] = [
    "core.sqlite3",
    "core.sqlite3-journal",
    "core.sqlite3-wal",
    "core.sqlite3-shm",
];

pub(crate) struct QrCandidateStore;

impl QrCandidateStore {
    pub(crate) fn create_marker(directory: &Path, nonce: &str) -> Result<()> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(directory.join(MARKER))?;
        file.write_all(format!("open:{nonce}").as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    fn marker(directory: &Path) -> Result<Option<File>> {
        let path = directory.join(MARKER);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => (),
            Ok(_) => return Err(Error::State("invalid QR candidate ownership marker")),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        Ok(Some(OpenOptions::new().read(true).write(true).open(path)?))
    }

    fn read(file: &mut File) -> Result<String> {
        // Long or incomplete records are never authority to open or delete.
        let mut value = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.take(38).read_to_string(&mut value)?;
        let Some(nonce) = value.strip_prefix("open:") else {
            return Err(Error::State("QR candidate ownership is invalid or retired"));
        };
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::State("invalid QR candidate ownership marker"));
        }
        Ok(value)
    }

    pub(super) fn open_lease(directory: &Path) -> Result<Option<File>> {
        let Some(mut marker) = Self::marker(directory)? else {
            return Ok(None);
        };
        marker.try_lock_shared().map_err(lock_error)?;
        Self::read(&mut marker)?;
        Ok(Some(marker))
    }

    pub(crate) fn retire(directory: &Path, nonce: &str) -> Result<()> {
        let mut marker = Self::marker(directory)?.ok_or(Error::StaleResponse)?;
        marker.try_lock().map_err(lock_error)?;
        if Self::read(&mut marker)? != format!("open:{nonce}") {
            return Err(Error::StaleResponse);
        }
        // Inspect only the exact newly owned directory. An unfamiliar entry,
        // including a link under an expected name, cancels the entire cleanup.
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            if !entry.file_type()?.is_file()
                || !(name == MARKER || DATABASE_FILES.iter().any(|known| name == *known))
            {
                return Err(Error::State("unexpected QR candidate store contents"));
            }
        }
        // Publish the tombstone under the exclusive lease before deleting.
        // A partial failure is fail closed; no startup scan or retry follows.
        marker.seek(SeekFrom::Start(0))?;
        marker.write_all(b"retired")?;
        marker.set_len(7)?;
        marker.sync_all()?;
        for name in DATABASE_FILES {
            let path = directory.join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() => fs::remove_file(path)?,
                Ok(_) => return Err(Error::State("invalid QR candidate database path")),
                Err(error) if error.kind() == ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

fn lock_error(error: TryLockError) -> Error {
    match error {
        TryLockError::WouldBlock => Error::MutationBusy,
        TryLockError::Error(error) => error.into(),
    }
}
