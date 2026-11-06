#![cfg(any(all(target_os = "linux", target_arch = "x86_64"), windows))]

use cumulus_core::{
    Result,
    library::{AudioMetadata, ListOptions, LocalLibrary, LocalResource, probe::AudioProbe},
    playback::engine::Cancellation,
    storage::StateStore,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const FLAC: &[u8] = include_bytes!("fixtures/stereo-pcm-source.flac");

struct Probe;

impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
        // The receipt must disregard this nominal format and geometry.
        Ok(AudioMetadata {
            format: Some("flac".into()),
            duration: 10.0,
            title: String::new(),
            album: String::new(),
            artists: Vec::new(),
            bitrate: None,
            sample_rate: Some(44_100),
            channels: Some(2),
            container: "flac".into(),
            cover_bytes: None,
        })
    }
}

fn setup() -> (tempfile::TempDir, PathBuf, LocalLibrary) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    fs::create_dir(&root).unwrap();
    let store = StateStore::open(temp.path().join("state")).unwrap();
    let library = LocalLibrary::with_probe(Arc::new(store), Arc::new(Probe)).unwrap();
    (temp, root, library)
}

fn indexed(library: &LocalLibrary, path: &Path) -> LocalResource {
    let id = cumulus_core::library::identity::file_id(path).unwrap();
    library.resource(&id).unwrap()
}

#[test]
fn cue_and_ncm_rows_never_make_a_whole_file_receipt() {
    let (_temp, root, library) = setup();
    let audio = root.join("source.flac");
    let ncm = root.join("protected.ncm");
    let renamed_ncm = root.join("not-really.flac");
    fs::write(&audio, FLAC).unwrap();
    fs::write(&ncm, b"CTENFDAMnot a plain FLAC").unwrap();
    fs::copy(&ncm, &renamed_ncm).unwrap();
    fs::write(
        root.join("chapters.cue"),
        "FILE source.flac WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    library.scan(&root).unwrap();
    let cue = library
        .list(ListOptions::default())
        .unwrap()
        .into_iter()
        .find(|row| {
            matches!(
                row.revision,
                cumulus_core::library::LocalRevision::Cue { .. }
            )
        })
        .expect("genuine indexed CUE row");
    let cue = library.resource(&cue.id).unwrap();
    let cancel = Cancellation::default();
    for selected in [
        cue,
        indexed(&library, &ncm),
        indexed(&library, &renamed_ncm),
    ] {
        assert!(
            library
                .hold_indexed_flac(&selected, &cancel)
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn same_name_same_size_and_mtime_cannot_redirect_the_held_receipt() {
    use cumulus_core::Error;
    use std::{fs::FileTimes, io::Read};

    let (temp, root, library) = setup();
    let audio = root.join("source.flac");
    fs::write(&audio, FLAC).unwrap();
    library.scan(&root).unwrap();
    let selected = indexed(&library, &audio);
    let cancel = Cancellation::default();
    let held = library
        .hold_indexed_flac(&selected, &cancel)
        .unwrap()
        .expect("plain indexed FLAC");
    let old = temp.path().join("held-original.flac");
    fs::rename(&audio, &old).unwrap();
    let mut other = FLAC.to_vec();
    let last = other.last_mut().unwrap();
    *last ^= 1;
    fs::write(&audio, &other).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&audio)
        .unwrap()
        .set_times(FileTimes::new().set_modified(fs::metadata(old).unwrap().modified().unwrap()))
        .unwrap();
    assert!(matches!(
        library.hold_indexed_flac(&selected, &cancel),
        Err(Error::QueueChanged)
    ));

    let mut actual = Vec::new();
    held.into_file(&selected, &cancel)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, FLAC);
    assert_eq!(fs::read(&audio).unwrap(), other);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn different_selection_rescan_and_original_cancellation_all_revoke_delivery() {
    use cumulus_core::Error;

    let (_temp, root, library) = setup();
    let audio = root.join("source.flac");
    let other = root.join("other.flac");
    fs::write(&audio, FLAC).unwrap();
    fs::write(&other, FLAC).unwrap();
    library.scan(&root).unwrap();
    let selected = indexed(&library, &audio);
    let other = indexed(&library, &other);
    let current = Cancellation::default();
    let hold = |cancel| {
        library
            .hold_indexed_flac(&selected, cancel)
            .unwrap()
            .unwrap()
    };
    assert!(matches!(
        hold(&current).into_file(&other, &current),
        Err(Error::QueueChanged)
    ));

    let original = Cancellation::default();
    let cancelled = hold(&original);
    original.cancel();
    assert!(matches!(
        cancelled.into_file(&selected, &current),
        Err(Error::Cancelled)
    ));

    let held = hold(&current);
    fs::write(
        &audio,
        include_bytes!("fixtures/stereo-pcm-source-48k.flac"),
    )
    .unwrap();
    assert_eq!(library.scan(&root).unwrap().updated, 1);
    let updated = indexed(&library, &audio);
    assert_ne!(updated.local_revision, selected.local_revision);
    assert!(matches!(
        library.hold_indexed_flac(&selected, &current),
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        held.into_file(&updated, &current),
        Err(Error::QueueChanged)
    ));
    assert!(
        library
            .hold_indexed_flac(&updated, &current)
            .unwrap()
            .is_some()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn substituted_symlink_to_the_same_inode_is_not_an_indexed_path() {
    use cumulus_core::Error;

    let (temp, root, library) = setup();
    let audio = root.join("source.flac");
    fs::write(&audio, FLAC).unwrap();
    library.scan(&root).unwrap();
    let selected = indexed(&library, &audio);
    let moved = temp.path().join("original.flac");
    fs::rename(&audio, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &audio).unwrap();
    assert!(matches!(
        library.hold_indexed_flac(&selected, &Cancellation::default()),
        Err(Error::QueueChanged)
    ));
}

#[cfg(windows)]
#[test]
fn legacy_windows_creation_timestamp_cannot_admit_a_flac_receipt() {
    let (_temp, root, library) = setup();
    let audio = root.join("source.flac");
    fs::write(&audio, FLAC).unwrap();
    library.scan(&root).unwrap();
    let selected = indexed(&library, &audio);
    // There is no strong Windows identity in this row. Even a disappeared
    // pathname does not reach an open/metadata preflight.
    fs::remove_file(&audio).unwrap();
    assert!(
        library
            .hold_indexed_flac(&selected, &Cancellation::default())
            .unwrap()
            .is_none()
    );
}
