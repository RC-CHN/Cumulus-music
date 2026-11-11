use super::*;
use crate::{
    library::{
        AudioMetadata,
        identity::{file_id, fingerprint},
        probe::AudioProbe,
    },
    playback::engine::pcm_wave::tests::wave,
    storage::StateStore,
};
use std::{
    fs::{self, FileTimes},
    os::windows::fs::FileTimesExt,
    sync::Arc,
};

struct Probe;

impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("pcm_s16le".into()),
            duration: 1.0,
            title: "Indexed Windows test WAV".into(),
            album: String::new(),
            artists: Vec::new(),
            bitrate: Some(1_411_200),
            sample_rate: Some(44_100),
            channels: Some(2),
            container: "wav".into(),
            cover_bytes: None,
        })
    }
}

#[test]
#[ignore = "requires Windows x64 and a real fixed local NTFS temp directory"]
fn windows_indexed_wave_keeps_scan_identity_and_refuses_a_legacy_or_replaced_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("music");
    fs::create_dir(&root).unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let library = LocalLibrary::with_probe(store, Arc::new(Probe)).unwrap();
    let path = root.join("合成 Indexed.WAV");
    let original = wave([[1, -1]; 1024]);
    let replaced = wave([[2, -2]; 1024]);
    fs::write(&path, &original).unwrap();
    let legacy = fingerprint(&path).unwrap();
    let times = fs::metadata(&path).unwrap();
    library.scan(&root).unwrap();
    let id = file_id(&path).unwrap();

    // Simulate an existing index before the Windows witness was introduced.
    let mut weak = library.item(&id).unwrap();
    weak.revision = LocalRevision::File {
        fingerprint: legacy.clone(),
    };
    library
        .store
        .put(OWNER, FILES, &id, &serde_json::to_value(weak).unwrap())
        .unwrap();
    let legacy_row = library.resource(&id).unwrap();
    let cancel = Cancellation::default();
    let acquire =
        |row| library.hold_ordinary_indexed_audio(row, &path, IndexedAudioFormat::Wave, &cancel);
    assert!(acquire(&legacy_row).unwrap().is_none());

    // A scan, not playback, changes the selected revision on the same file.
    library.scan(&root).unwrap();
    let current = library.resource(&id).unwrap();
    assert_ne!(
        current.local_revision.token(),
        legacy_row.local_revision.token()
    );
    let held = acquire(&current)
        .unwrap()
        .expect("rooted scan-owned indexed WAV");
    fs::rename(&path, tmp.path().join("original.wav")).unwrap();
    fs::write(&path, &replaced).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            FileTimes::new()
                .set_created(times.created().unwrap())
                .set_modified(times.modified().unwrap()),
        )
        .unwrap();
    assert_eq!(fingerprint(&path).unwrap(), legacy);
    assert!(matches!(acquire(&current), Err(Error::QueueChanged)));
    assert!(library.resolve(&current).is_err());

    let mut bytes = Vec::new();
    held.into_ordinary_file(&current, &cancel)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, original);
    assert_eq!(fs::read(&path).unwrap(), replaced);
    library.scan(&root).unwrap();
    assert_ne!(
        library.resource(&id).unwrap().local_revision,
        current.local_revision
    );
    assert!(matches!(acquire(&current), Err(Error::QueueChanged)));
}

#[test]
#[ignore = "requires Windows x64 and a real fixed local NTFS temp directory"]
fn windows_indexed_wave_does_not_adopt_a_late_download_root_alias() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("音频");
    fs::create_dir(&root).unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let library = LocalLibrary::with_probe(store, Arc::new(Probe)).unwrap();
    let path = root.join("Source.wav");
    fs::write(&path, wave([[0, 0]; 512])).unwrap();
    library.scan(&root).unwrap();
    let id = file_id(&path).unwrap();
    let selected = library.resource(&id).unwrap();
    let cancel = Cancellation::default();
    let acquire =
        || library.hold_ordinary_indexed_audio(&selected, &path, IndexedAudioFormat::Wave, &cancel);
    let held = acquire().unwrap().unwrap();
    let name = root.to_str().unwrap();
    let alias = name
        .strip_prefix(r"\\?\")
        .unwrap_or(name)
        .to_ascii_lowercase();
    library
        .store
        .put(
            "another-owner",
            "download_directory_v1",
            "settings",
            &serde_json::json!({"root": alias}),
        )
        .unwrap();
    assert!(acquire().unwrap().is_none());
    assert!(matches!(
        held.into_ordinary_file(&selected, &cancel),
        Err(Error::StaleResponse)
    ));
    library.scan(&root).unwrap();
    assert_eq!(
        library.resource(&id).unwrap().local_revision,
        LocalRevision::File {
            fingerprint: fingerprint(&path).unwrap()
        }
    );
}
