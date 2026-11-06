//! An optional native handoff from the exact indexed local file revision.

use super::{FILES, LocalLibrary, OWNER};
use crate::{
    Error, Result,
    library::{
        LibraryItem, LocalResource, LocalRevision,
        identity::{FileFingerprint, fingerprint_metadata, path_id},
        native_open::safe_path,
    },
    playback::engine::Cancellation,
};
use std::{
    fs::File,
    io::{ErrorKind, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Owns exactly the opened, fingerprint-checked plain FLAC descriptor.
/// This is not a PCM format observation, account grant, or decoder approval.
/// It has no clone, pathname getter, or persisted representation.
pub struct IndexedFlacReceipt {
    library: LocalLibrary,
    id: String,
    path: PathBuf,
    fingerprint: FileFingerprint,
    prepare: Cancellation,
    file: File,
}

impl LocalLibrary {
    /// Prepare a native-only handoff for a whole, directly indexed FLAC file.
    /// Use on a blocking worker. No queue metadata or renderer-provided path
    /// chooses the file. Unsupported origins/platform identity return `None`;
    /// a stale indexed revision is an error and cannot be opened as a new row.
    pub fn hold_indexed_flac(
        &self,
        selected: &LocalResource,
        cancel: &Cancellation,
    ) -> Result<Option<IndexedFlacReceipt>> {
        cancel.check()?;
        let LocalRevision::File { fingerprint } = &selected.local_revision else {
            return Ok(None); // CUE virtual tracks are never whole-file candidates.
        };
        let path = self.indexed_flac_path(selected, fingerprint)?;
        if !safe_path(&path)
            || path.starts_with(&self.state_root)
            || !path
                .extension()
                .is_some_and(|value| value.eq_ignore_ascii_case("flac"))
        {
            return Ok(None); // Do not resolve NCM, decode a cache, or follow a fallback path.
        }
        cancel.check()?;
        let Some(mut file) = open_indexed_file(&path, fingerprint)? else {
            return Ok(None);
        };
        check_fingerprint(&file, fingerprint)?;
        // Only bound container identification, not sample format. In
        // particular, never pass a renamed NCM to a decryptor here.
        let mut marker = [0; 4];
        let is_flac = match file.read_exact(&mut marker) {
            Ok(()) => marker == *b"fLaC",
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => false,
            Err(error) => return Err(error.into()),
        };
        file.seek(SeekFrom::Start(0))?;
        check_fingerprint(&file, fingerprint)?;
        if self.indexed_flac_path(selected, fingerprint)? != path {
            return Err(Error::QueueChanged);
        }
        cancel.check()?;
        Ok(is_flac.then(|| IndexedFlacReceipt {
            library: self.clone(),
            id: selected.local_id.clone(),
            path,
            fingerprint: fingerprint.clone(),
            prepare: cancel.clone(),
            file,
        }))
    }

    fn indexed_flac_path(
        &self,
        selected: &LocalResource,
        fingerprint: &FileFingerprint,
    ) -> Result<PathBuf> {
        let id = &selected.local_id;
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid indexed FLAC id"));
        }
        // A single durable file row, not a full library scan or the CUE index.
        let row = self
            .store
            .get(OWNER, FILES, id)?
            .ok_or(Error::QueueChanged)?;
        let row: LibraryItem = serde_json::from_value(row)?;
        if &row.id != id
            || !row.available()
            || row.revision != selected.local_revision
            || !matches!(&row.revision, LocalRevision::File { fingerprint: actual } if actual == fingerprint)
            || !row.path.is_absolute()
            || path_id(&row.path)? != *id
        {
            return Err(Error::QueueChanged);
        }
        Ok(row.path)
    }
}

impl IndexedFlacReceipt {
    /// Move the same descriptor exactly once after rechecking the current
    /// selected resource and durable index. Neither this step nor a pathname
    /// replacement can open another file. The native caller still owns its
    /// account, queue, output and ongoing decoder lifetime checks.
    pub fn into_file(self, selected: &LocalResource, cancel: &Cancellation) -> Result<File> {
        self.prepare.check()?;
        cancel.check()?;
        if selected.local_id != self.id
            || self
                .library
                .indexed_flac_path(selected, &self.fingerprint)?
                != self.path
        {
            return Err(Error::QueueChanged);
        }
        check_fingerprint(&self.file, &self.fingerprint)?;
        self.prepare.check()?;
        cancel.check()?;
        Ok(self.file)
    }
}

fn check_fingerprint(file: &File, expected: &FileFingerprint) -> Result<()> {
    if fingerprint_metadata(file.metadata()?)? != *expected {
        return Err(Error::QueueChanged);
    }
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn open_indexed_file(path: &Path, expected: &FileFingerprint) -> Result<Option<File>> {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

    // The Linux index saves the device and inode from Metadata. A legacy
    // missing identity must not qualify on size and timestamps alone.
    if expected
        .file_identity
        .as_deref()
        .is_none_or(|id| !id.starts_with("unix:"))
    {
        return Ok(None);
    }
    if path.canonicalize()? != path {
        return Err(Error::QueueChanged);
    }
    // Do not follow a leaf that was swapped to a symlink or block on a FIFO
    // inserted after the pathname check. The caller checks the actual opened
    // descriptor's inode before reading, including any parent path race.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    Ok(Some(file))
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn open_indexed_file(_path: &Path, _expected: &FileFingerprint) -> Result<Option<File>> {
    // The current Windows index may contain creation time, not a device/file
    // ID taken from this handle. Do not open or try to prove identity by path.
    Ok(None)
}
