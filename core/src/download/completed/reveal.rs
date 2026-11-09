//! Read-only file-manager handoff for an exact published download.
use super::{CompletedDownloads, fs};
use crate::{
    Error, Result,
    account::SessionToken,
    download::{
        offline::{OfflineLibrary, OfflineRecord},
        service::{DownloadPhase, files},
    },
    library::identity::fingerprint_metadata,
    media::integrity::FileIntegrity,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self as std_fs, Metadata, OpenOptions},
    io::Read,
    path::Path,
};

/// Native-only proof of the exact file to select. The renderer never supplies
/// a path or receives this receipt.
pub struct DownloadFileReveal {
    downloads: CompletedDownloads,
    token: SessionToken,
    record: OfflineRecord,
    facts: fs::Facts,
}

impl DownloadFileReveal {
    pub fn path(&self) -> &Path {
        &self.record.path
    }

    /// Recheck immediately before the host dispatches the path to its shell.
    /// The OS file manager takes a pathname, not the verified file handle.
    pub fn check_current(&self) -> Result<()> {
        let record = self
            .downloads
            .reveal_publication(&self.token, &self.record.task_id)?;
        if record != self.record {
            return Err(Error::StaleResponse);
        }
        fs::unchanged(self.path(), &self.facts)?;
        self.downloads.guard(&Some(self.token.clone()))
    }
}

impl CompletedDownloads {
    fn reveal_publication(&self, token: &SessionToken, id: &str) -> Result<OfflineRecord> {
        self.guard(&Some(token.clone()))?;
        let task = self.task(id)?;
        let path = task.playback_target()?;
        if task.phase != DownloadPhase::Complete
            || files::target(&task)? != path
            || !files::published(&task)?
            || path.canonicalize()? != path
        {
            return Err(Error::State("download publication changed"));
        }
        check_shape(&std_fs::symlink_metadata(path)?)?;
        let integrity = task
            .output_integrity
            .as_ref()
            .ok_or(Error::State("download publication integrity is missing"))?;
        integrity.validate()?;
        let session = self.session.as_ref().ok_or(Error::StaleResponse)?;
        let record = OfflineLibrary::new(session.clone())?
            .record_at_verified_path(token, path)?
            .ok_or(Error::State("download publication record is missing"))?;
        if record.task_id != id
            || record.song_id != task.song_id
            || record.encrypted != task.encrypted
            || &record.integrity != integrity
        {
            return Err(Error::State("download publication record changed"));
        }
        Ok(record)
    }

    pub(crate) fn verify_reveal(&self, id: &str) -> Result<DownloadFileReveal> {
        let token = self.token()?.ok_or(Error::StaleResponse)?;
        let record = self.reveal_publication(&token, id)?;
        let facts = fs::recorded_facts(&record.path, record.integrity.clone())?;
        verify_bytes(&record.path, &record.integrity, &facts)?;
        let reveal = DownloadFileReveal {
            downloads: self.clone(),
            token,
            record,
            facts,
        };
        reveal.check_current()?;
        Ok(reveal)
    }
}

fn check_shape(metadata: &Metadata) -> Result<()> {
    if !metadata.is_file() {
        return Err(Error::State("download publication is not a regular file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::State("download publication is a reparse point"));
        }
    }
    Ok(())
}

fn verify_bytes(path: &Path, expected: &FileIntegrity, facts: &fs::Facts) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    // Reopen the pathname with the same restrictions for each identity check.
    // An unguarded reopen could follow a replacement symlink or block on a FIFO.
    let open = || {
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        check_shape(&metadata)?;
        Ok::<_, Error>((file, metadata))
    };
    let (mut file, opened) = open()?;
    let held = same_file::Handle::from_file(file.try_clone()?)?;
    if held != same_file::Handle::from_file(open()?.0)?
        || fingerprint_metadata(opened)? != facts.fingerprint
    {
        return Err(Error::State("download publication changed while opening"));
    }
    let mut hash = Sha256::new();
    // Bound the read by the recorded publication length (one extra byte detects growth).
    let bytes = std::io::copy(
        &mut file.by_ref().take(expected.bytes.saturating_add(1)),
        &mut hash,
    )?;
    if bytes != expected.bytes
        || hex::encode(hash.finalize()) != expected.sha256
        || held != same_file::Handle::from_file(open()?.0)?
        || fingerprint_metadata(file.metadata()?)? != facts.fingerprint
    {
        return Err(Error::Media(
            "download publication failed integrity verification",
        ));
    }
    Ok(())
}
