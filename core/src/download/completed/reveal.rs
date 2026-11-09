//! Read-only file-manager handoff for an exact published download.
use super::{CompletedDownloads, fs};
use crate::{
    Error, Result,
    account::SessionToken,
    download::{
        offline::{OfflineLibrary, OfflineRecord},
        service::{DownloadOrigin, DownloadPhase, files},
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
    origin: DownloadOrigin,
    facts: fs::Facts,
}

/// A menu-time pin using the existing publication receipt, before hashing bytes.
pub(crate) struct DownloadFileCandidate(DownloadFileReveal);

impl DownloadFileCandidate {
    pub(crate) fn verify(self) -> Result<DownloadFileReveal> {
        let reveal = self.0;
        verify_reveal_bytes(reveal.path(), &reveal.record.integrity, &reveal.facts)?;
        reveal.check_current()?;
        Ok(reveal)
    }
}

impl DownloadFileReveal {
    pub fn path(&self) -> &Path {
        &self.record.path
    }

    pub(crate) fn matches_cloud_association(
        &self,
        cloud_id: &str,
        catalog_id: Option<&str>,
    ) -> bool {
        match &self.origin {
            DownloadOrigin::Cloud { .. } => self.record.song_id == cloud_id,
            DownloadOrigin::Song => catalog_id == Some(self.record.song_id.as_str()),
            _ => false,
        }
    }

    pub(crate) fn matches_song_association(&self, media_id: &str) -> bool {
        self.record.song_id == media_id && matches!(&self.origin, DownloadOrigin::Song)
    }

    pub(crate) fn matches_voice_association(&self, voice_id: &str, media_id: &str) -> bool {
        self.record.song_id == media_id
            && matches!(&self.origin, DownloadOrigin::Voice { voice_id: id } if id == voice_id)
    }

    /// Recheck immediately before the host dispatches the path to its shell.
    /// The OS file manager takes a pathname, not the verified file handle.
    pub fn check_current(&self) -> Result<()> {
        let (record, origin) = self
            .downloads
            .reveal_publication(&self.token, &self.record.task_id)?;
        if record != self.record || origin != self.origin {
            return Err(Error::StaleResponse);
        }
        fs::unchanged(self.path(), &self.facts)?;
        self.downloads.guard(&Some(self.token.clone()))
    }
}

impl CompletedDownloads {
    fn reveal_publication(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<(OfflineRecord, DownloadOrigin)> {
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
        Ok((record, task.origin))
    }

    /// Cheap indexed candidate check; dispatch still hashes the selected file.
    pub(crate) fn voice_reveal_candidate(
        &self,
        id: &str,
        voice_id: &str,
        media_id: &str,
    ) -> Result<std::path::PathBuf> {
        let token = self.token()?.ok_or(Error::StaleResponse)?;
        let (record, origin) = self.reveal_publication(&token, id)?;
        if record.song_id != media_id
            || !matches!(&origin, DownloadOrigin::Voice { voice_id: id } if id == voice_id)
            || std_fs::symlink_metadata(&record.path)?.len() != record.integrity.bytes
        {
            return Err(Error::DownloadedFileMissing);
        }
        Ok(record.path)
    }

    pub(crate) fn verify_reveal(&self, id: &str) -> Result<DownloadFileReveal> {
        self.verify_reveal_at(id, None)
    }

    pub(crate) fn verify_reveal_at(
        &self,
        id: &str,
        path: Option<&Path>,
    ) -> Result<DownloadFileReveal> {
        self.prepare_reveal_at(id, path)?.verify()
    }

    pub(crate) fn catalog_song_reveal_candidate(
        &self,
        id: &str,
        song: &str,
        path: &Path,
    ) -> Result<DownloadFileCandidate> {
        let candidate = self.prepare_reveal_at(id, Some(path))?;
        if !candidate.0.matches_song_association(song) {
            return Err(Error::QueueChanged);
        }
        candidate.0.check_current()?;
        Ok(candidate)
    }

    fn prepare_reveal_at(&self, id: &str, path: Option<&Path>) -> Result<DownloadFileCandidate> {
        let token = self.token()?.ok_or(Error::StaleResponse)?;
        let (record, origin) = self.reveal_publication(&token, id)?;
        if path.is_some_and(|expected| record.path != expected) {
            return Err(Error::QueueChanged);
        }
        let facts = fs::recorded_facts(&record.path, record.integrity.clone())?;
        Ok(DownloadFileCandidate(DownloadFileReveal {
            downloads: self.clone(),
            token,
            record,
            origin,
            facts,
        }))
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

pub(crate) fn verify_reveal_bytes(
    path: &Path,
    expected: &FileIntegrity,
    facts: &fs::Facts,
) -> Result<()> {
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
