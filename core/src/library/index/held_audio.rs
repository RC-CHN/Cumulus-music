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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexedAudioFormat {
    Flac,
    Wave,
}

impl IndexedAudioFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?;
        if ext.eq_ignore_ascii_case("flac") {
            Some(Self::Flac)
        } else if ext.eq_ignore_ascii_case("wav") {
            Some(Self::Wave)
        } else {
            None
        }
    }

    pub(crate) fn demuxer(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Wave => "wav",
        }
    }

    pub(super) fn check_container(self, file: &mut File) -> Result<bool> {
        let mut header = [0; 12];
        let bytes = match self {
            Self::Flac => &mut header[..4],
            Self::Wave => &mut header[..],
        };
        match file.read_exact(bytes) {
            Ok(()) => (),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error.into()),
        }
        Ok(match self {
            Self::Flac => &header[..4] == b"fLaC",
            Self::Wave => &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        })
    }
}

/// Owns exactly the opened, fingerprint-checked plain audio descriptor.
/// This is not a PCM format observation, account grant, or decoder approval.
/// It has no clone, pathname getter, or persisted representation.
pub struct IndexedAudioReceipt {
    format: IndexedAudioFormat,
    library: LocalLibrary,
    id: String,
    path: PathBuf,
    fingerprint: FileFingerprint,
    prepare: Cancellation,
    file: File,
}

/// Backward-compatible name for receipts obtained through `hold_indexed_flac`.
pub type IndexedFlacReceipt = IndexedAudioReceipt;

impl LocalLibrary {
    pub(crate) fn ordinary_indexed_audio_current(
        &self,
        selected: &LocalResource,
        path: &Path,
        format: IndexedAudioFormat,
    ) -> Result<bool> {
        let LocalRevision::File { fingerprint } = &selected.local_revision else {
            return Ok(false);
        };
        #[cfg(windows)]
        if format != IndexedAudioFormat::Wave || !super::windows_wave::witnessed(fingerprint) {
            return Ok(false);
        }
        let indexed = self.indexed_audio_path(selected, fingerprint)?;
        if indexed != path || IndexedAudioFormat::from_path(&indexed) != Some(format) {
            // An indexed NCM can legitimately resolve to a different FLAC
            // cache pathname. It keeps the original stock route.
            return Ok(false);
        }
        let Some(path) = path.to_str() else {
            return Ok(false);
        };
        #[cfg(windows)]
        let claimed = self
            .store
            .download_claims_indexed_windows_path(&selected.local_id, path)?;
        #[cfg(not(windows))]
        let claimed = self
            .store
            .download_claims_indexed_path(&selected.local_id, path)?;
        Ok(!claimed)
    }

    pub(crate) fn hold_ordinary_indexed_audio(
        &self,
        selected: &LocalResource,
        path: &Path,
        format: IndexedAudioFormat,
        cancel: &Cancellation,
    ) -> Result<Option<IndexedAudioReceipt>> {
        if !cfg!(all(
            any(target_os = "linux", windows),
            target_arch = "x86_64"
        )) || !self.ordinary_indexed_audio_current(selected, path, format)?
        {
            return Ok(None);
        }
        let Some(held) = self.hold_indexed_audio(selected, format, cancel)? else {
            return Ok(None);
        };
        if held.path != path || !single_link(&held.file, &held.fingerprint)? {
            return Ok(None);
        }
        Ok(Some(held))
    }

    /// Prepare a native-only handoff for a whole, directly indexed FLAC file.
    /// Use on a blocking worker. No queue metadata or renderer-provided path
    /// chooses the file. Unsupported origins/platform identity return `None`;
    /// a stale indexed revision is an error and cannot be opened as a new row.
    pub fn hold_indexed_flac(
        &self,
        selected: &LocalResource,
        cancel: &Cancellation,
    ) -> Result<Option<IndexedFlacReceipt>> {
        self.hold_indexed_audio(selected, IndexedAudioFormat::Flac, cancel)
    }

    fn hold_indexed_audio(
        &self,
        selected: &LocalResource,
        format: IndexedAudioFormat,
        cancel: &Cancellation,
    ) -> Result<Option<IndexedAudioReceipt>> {
        cancel.check()?;
        let LocalRevision::File { fingerprint } = &selected.local_revision else {
            return Ok(None); // CUE virtual tracks are never whole-file candidates.
        };
        let path = self.indexed_audio_path(selected, fingerprint)?;
        if !safe_path(&path)
            || path.starts_with(&self.state_root)
            || IndexedAudioFormat::from_path(&path) != Some(format)
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
        let matches_container = format.check_container(&mut file)?;
        file.seek(SeekFrom::Start(0))?;
        #[cfg(windows)]
        if format == IndexedAudioFormat::Wave {
            if !matches_container {
                return Err(Error::QueueChanged);
            }
            use crate::library::matching::MatchHint;
            let same_handle_gain = crate::library::matching::opened_metadata_hint(
                &mut file,
                false,
                crate::media::ncm::HeaderPolicy::Compatible,
            );
            if match same_handle_gain {
                Ok(MatchHint::Absent) => false,
                Ok(MatchHint::Song(info)) => !info["volumeDelta"].is_null(),
                Ok(MatchHint::Invalid) | Err(_) => true,
            } {
                // The ordinary path already reported no competing gain;
                // a mismatch on the actual descriptor is stale, not a
                // reason to load a pathname after this acquisition.
                return Err(Error::StaleResponse);
            }
        }
        #[cfg(windows)]
        file.seek(SeekFrom::Start(0))?;
        check_fingerprint(&file, fingerprint)?;
        if self.indexed_audio_path(selected, fingerprint)? != path {
            return Err(Error::QueueChanged);
        }
        cancel.check()?;
        Ok(matches_container.then(|| IndexedAudioReceipt {
            format,
            library: self.clone(),
            id: selected.local_id.clone(),
            path,
            fingerprint: fingerprint.clone(),
            prepare: cancel.clone(),
            file,
        }))
    }

    fn indexed_audio_path(
        &self,
        selected: &LocalResource,
        fingerprint: &FileFingerprint,
    ) -> Result<PathBuf> {
        let id = &selected.local_id;
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid indexed audio id"));
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

impl IndexedAudioReceipt {
    pub fn format(&self) -> IndexedAudioFormat {
        self.format
    }

    pub(crate) fn into_ordinary_file(
        self,
        selected: &LocalResource,
        cancel: &Cancellation,
    ) -> Result<File> {
        if !single_link(&self.file, &self.fingerprint)?
            || !self
                .library
                .ordinary_indexed_audio_current(selected, &self.path, self.format)?
        {
            return Err(Error::StaleResponse);
        }
        self.into_file(selected, cancel)
    }

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
                .indexed_audio_path(selected, &self.fingerprint)?
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

#[cfg(unix)]
fn single_link(file: &File, _: &FileFingerprint) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink() == 1)
}

#[cfg(windows)]
fn single_link(file: &File, expected: &FileFingerprint) -> Result<bool> {
    // The Windows identity comparison includes a current single live link.
    super::windows_wave::matches(file, expected)
}

#[cfg(not(any(unix, windows)))]
fn single_link(_file: &File, _: &FileFingerprint) -> Result<bool> {
    Ok(false)
}

fn check_fingerprint(file: &File, expected: &FileFingerprint) -> Result<()> {
    #[cfg(windows)]
    if super::windows_wave::witnessed(expected) {
        return if super::windows_wave::matches(file, expected)? {
            Ok(())
        } else {
            Err(Error::QueueChanged)
        };
    }
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

#[cfg(all(windows, target_arch = "x86_64"))]
fn open_indexed_file(path: &Path, expected: &FileFingerprint) -> Result<Option<File>> {
    if !super::windows_wave::witnessed(expected) {
        return Ok(None);
    }
    let (file, _) =
        crate::library::identity::windows::WindowsFileIdentity::open_local_fixed_ntfs_nofollow(
            path,
        )?
        .ok_or(Error::QueueChanged)?;
    Ok(Some(file))
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(windows, target_arch = "x86_64")
)))]
fn open_indexed_file(_path: &Path, _expected: &FileFingerprint) -> Result<Option<File>> {
    Ok(None)
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::{
        library::{AudioMetadata, ListOptions, identity::file_id, probe::AudioProbe},
        storage::StateStore,
    };
    use serde_json::json;
    use std::{fs, sync::Arc};

    struct Probe;

    impl AudioProbe for Probe {
        fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
            // Metadata never decides the format of the held input.
            Ok(AudioMetadata {
                format: Some("flac".into()),
                duration: 1.0,
                title: String::new(),
                album: String::new(),
                artists: Vec::new(),
                bitrate: None,
                sample_rate: None,
                channels: None,
                container: "flac".into(),
                cover_bytes: None,
            })
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, LocalLibrary) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("music");
        fs::create_dir(&root).unwrap();
        let store = StateStore::open(temp.path().join("state")).unwrap();
        let library = LocalLibrary::with_probe(Arc::new(store), Arc::new(Probe)).unwrap();
        (temp, root, library)
    }

    fn selection(library: &LocalLibrary, path: &Path) -> LocalResource {
        library.resource(&file_id(path).unwrap()).unwrap()
    }

    fn wave(sample: u8) -> [u8; 48] {
        let mut bytes = *b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x44\xac\0\0\x10\xb1\x02\0\x04\0\x10\0data\x04\0\0\0\0\0\0\0";
        bytes[44] = sample;
        bytes
    }

    #[test]
    fn indexed_wave_receipt_keeps_original_descriptor_across_path_replacement() {
        let (temp, root, library) = fixture();
        let path = root.join("source.WAV");
        fs::write(&path, wave(1)).unwrap();
        library.scan(&root).unwrap();
        let selected = selection(&library, &path);
        let cancel = Cancellation::default();
        let acquire = || {
            library.hold_ordinary_indexed_audio(&selected, &path, IndexedAudioFormat::Wave, &cancel)
        };
        let held = acquire().unwrap().unwrap();
        assert_eq!(held.format(), IndexedAudioFormat::Wave);
        fs::rename(&path, temp.path().join("original.wav")).unwrap();
        fs::write(&path, wave(2)).unwrap();
        assert!(matches!(acquire(), Err(Error::QueueChanged)));
        let mut actual = Vec::new();
        held.into_ordinary_file(&selected, &cancel)
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, wave(1));
    }

    #[test]
    fn indexed_wave_excludes_cue_renamed_ncm_and_late_download_claim() {
        let (_temp, root, library) = fixture();
        let path = root.join("source.wav");
        let renamed = root.join("protected.wav");
        fs::write(&path, wave(1)).unwrap();
        fs::write(&renamed, b"CTENFDAMnot a WAV").unwrap();
        fs::write(
            root.join("source.cue"),
            "FILE source.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n",
        )
        .unwrap();
        library.scan(&root).unwrap();
        let cue = library
            .list(ListOptions::default())
            .unwrap()
            .into_iter()
            .find(|row| matches!(row.revision, LocalRevision::Cue { .. }))
            .unwrap();
        let cue = library.resource(&cue.id).unwrap();
        let cancel = Cancellation::default();
        let hold = |selected: &LocalResource, path: &Path| {
            library.hold_ordinary_indexed_audio(selected, path, IndexedAudioFormat::Wave, &cancel)
        };
        assert!(hold(&cue, &path).unwrap().is_none());
        assert!(
            hold(&selection(&library, &renamed), &renamed)
                .unwrap()
                .is_none()
        );
        let selected = selection(&library, &path);
        let held = hold(&selected, &path).unwrap().unwrap();
        library
            .store
            .put(
                "another-account",
                "download_tasks_v1",
                "claim",
                &json!({"target": path}),
            )
            .unwrap();
        assert!(hold(&selected, &path).unwrap().is_none());
        assert!(matches!(
            held.into_ordinary_file(&selected, &cancel),
            Err(Error::StaleResponse)
        ));
    }
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod windows_tests;
