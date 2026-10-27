mod support;
use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    library::{AudioMetadata, ListOptions, LocalLibrary, identity::file_id, probe::AudioProbe},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use support::ScriptApi;

#[derive(Default)]
struct Probe {
    calls: Mutex<Vec<PathBuf>>,
    change: AtomicBool,
}
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        self.calls.lock().unwrap().push(path.into());
        let text = std::fs::read_to_string(path)?;
        let duration = text
            .trim()
            .parse::<f64>()
            .map_err(|_| Error::Media("synthetic corrupt audio"))?;
        if self.change.swap(false, Ordering::SeqCst) {
            std::fs::write(path, "99 changed")?;
        }
        Ok(AudioMetadata {
            format: Some("wav".into()),
            duration,
            title: path.file_stem().unwrap().to_str().unwrap().into(),
            album: "Album".into(),
            artists: vec![],
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            container: "wav".into(),
            cover_bytes: None,
        })
    }
}
fn setup() -> (
    tempfile::TempDir,
    PathBuf,
    Arc<StateStore>,
    Arc<Probe>,
    LocalLibrary,
) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let probe = Arc::new(Probe::default());
    let library = LocalLibrary::with_probe(store.clone(), probe.clone()).unwrap();
    (tmp, root, store, probe, library)
}
async fn session(store: Arc<StateStore>, uid: u64) -> AccountSession {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":uid}}).into(),
    ]));
    let session = AccountSession::new(api, Some(store));
    session.refresh().await.unwrap();
    session
}

#[tokio::test]
async fn scans_reopen_search_and_account_mapping_track_actual_file_revisions() {
    let (_tmp, root, store, probe, library) = setup();
    let audio = root.join("Straße.wav");
    std::fs::write(&audio, "3").unwrap();
    assert_eq!(library.scan(&root).unwrap().updated, 1);
    assert_eq!(library.scan(&root).unwrap().unchanged, 1);
    assert_eq!(probe.calls.lock().unwrap().len(), 1);
    let list = library
        .list(ListOptions {
            query: "STRASSE",
            ..Default::default()
        })
        .unwrap();
    assert_eq!(list.len(), 1);
    let id = list[0].id.clone();
    let resource = library.resource(&id).unwrap();
    assert_eq!(
        library.resolve(&resource).unwrap().source,
        audio.canonicalize().unwrap()
    );
    let one = session(store.clone(), 1).await;
    let two = session(store.clone(), 2).await;
    library.map_online(&one, &id, &json!(123)).unwrap();
    assert_eq!(
        library.online_id(&one, &id).unwrap().as_deref(),
        Some("123")
    );
    assert!(library.online_id(&two, &id).unwrap().is_none());
    let reopened = LocalLibrary::with_probe(
        Arc::new(StateStore::open(store.directory()).unwrap()),
        probe,
    )
    .unwrap();
    assert_eq!(reopened.list(ListOptions::default()).unwrap().len(), 1);
    std::fs::write(&audio, "4 ").unwrap();
    assert!(reopened.online_id(&one, &id).unwrap().is_none());
    assert!(reopened.resolve(&resource).is_err());
    assert_eq!(reopened.scan(&root).unwrap().updated, 1);
    assert!(reopened.online_id(&one, &id).unwrap().is_none());
    assert!(reopened.resolve(&resource).is_err());
    reopened.remove(&id).unwrap();
    assert!(audio.exists());
    assert!(reopened.item(&id).is_err());
}

#[test]
fn unreadable_missing_recovered_files_and_state_exclusion_converge() {
    let (tmp, root, store, _, library) = setup();
    let audio = root.join("a.wav");
    std::fs::write(&audio, "3").unwrap();
    library.scan(&root).unwrap();
    let id = file_id(&audio).unwrap();
    std::fs::write(&audio, "bad").unwrap();
    let scan = library.scan(&root).unwrap();
    assert_eq!(scan.errors.len(), 1);
    assert!(library.item(&id).is_err());
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
    std::fs::write(&audio, "5 ").unwrap();
    assert_eq!(library.scan(&root).unwrap().updated, 1);
    std::fs::remove_file(&audio).unwrap();
    assert_eq!(library.scan(&root).unwrap().missing, 1);
    let all = library
        .list(ListOptions {
            include_missing: true,
            ..Default::default()
        })
        .unwrap();
    assert!(all[0].missing);
    std::fs::write(store.directory().join("cache.wav"), "7").unwrap();
    library.scan(tmp.path()).unwrap();
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
    #[cfg(unix)]
    {
        std::fs::write(tmp.path().join("outside.wav"), "4").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("outside.wav"), root.join("link.wav")).unwrap();
        library.scan(&root).unwrap();
        assert!(
            library
                .list(ListOptions::default())
                .unwrap()
                .iter()
                .all(|i| i.path != root.join("link.wav"))
        );
    }
}

fn sheet(second: &str) -> String {
    format!(
        "TITLE \"Album\"\nPERFORMER Artist\nFILE a.wav WAVE\nTRACK 01 AUDIO\nTITLE One\nINDEX 01 00:01:00\nTRACK 02 AUDIO\nTITLE Two\nINDEX 01 {second}\n"
    )
}
#[tokio::test]
async fn cue_revisions_are_independent_and_invalid_sheets_restore_container_visibility() {
    let (_tmp, root, store, _, library) = setup();
    let audio = root.join("a.wav");
    let cue = root.join("a.cue");
    std::fs::write(&audio, "5").unwrap();
    std::fs::write(&cue, sheet("00:03:00")).unwrap();
    let scan = library.scan(&root).unwrap();
    assert_eq!((scan.updated, scan.cue_updated), (1, 1));
    let tracks = library.list(ListOptions::default()).unwrap();
    assert_eq!(tracks.len(), 2);
    assert_ne!(tracks[0].id, tracks[1].id);
    assert_eq!(
        library
            .list(ListOptions {
                include_containers: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
        3
    );
    let first = library.resource(&tracks[0].id).unwrap();
    let resolved = library.resolve(&first).unwrap();
    assert_eq!(resolved.segment.unwrap().start, 1.0);
    let session = session(store, 1).await;
    let container = file_id(&audio).unwrap();
    library
        .map_online(&session, &container, &json!(103))
        .unwrap();
    assert_eq!(
        library.online_id(&session, &container).unwrap().as_deref(),
        Some("103")
    );
    library
        .map_online(&session, &tracks[0].id, &json!(101))
        .unwrap();
    library
        .map_online(&session, &tracks[1].id, &json!(102))
        .unwrap();
    assert_eq!(library.scan(&root).unwrap().cue_unchanged, 1);
    std::fs::write(&cue, sheet("00:04:00")).unwrap();
    assert!(
        library
            .online_id(&session, &tracks[0].id)
            .unwrap()
            .is_none()
    );
    assert_eq!(library.scan(&root).unwrap().cue_updated, 1);
    assert!(library.resolve(&first).is_err());
    library.remove(&tracks[0].id).unwrap();
    assert!(cue.exists() && audio.exists());
    assert!(library.item(&tracks[0].id).is_err());
    assert_eq!(library.scan(&root).unwrap().cue_updated, 1);
    std::fs::write(&cue, sheet("00:01:00")).unwrap();
    assert_eq!(library.scan(&root).unwrap().errors.len(), 1);
    let visible = library.list(ListOptions::default()).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, file_id(&audio).unwrap());
    std::fs::remove_file(cue).unwrap();
    assert_eq!(library.scan(root).unwrap().cue_missing, 1);
}

#[test]
fn cancellation_database_failure_and_changed_during_probe_do_not_publish_partial_scans() {
    let (_tmp, root, store, probe, library) = setup();
    let audio = root.join("a.wav");
    std::fs::write(&audio, "3").unwrap();
    library.scan(&root).unwrap();
    let baseline = store.values("local", "library_files_v1").unwrap();
    std::fs::write(&audio, "4 ").unwrap();
    let cancelled = || probe.calls.lock().unwrap().len() >= 2;
    assert!(matches!(
        library.scan_with(&root, &cancelled),
        Err(Error::Cancelled)
    ));
    assert_eq!(store.values("local", "library_files_v1").unwrap(), baseline);
    let connection = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_scan BEFORE INSERT ON state WHEN NEW.kind='library_files_v1' BEGIN SELECT RAISE(FAIL,'test failure'); END;").unwrap();
    assert!(matches!(library.scan(&root), Err(Error::Storage(_))));
    assert_eq!(store.values("local", "library_files_v1").unwrap(), baseline);
    connection.execute_batch("DROP TRIGGER fail_scan;").unwrap();
    probe.change.store(true, Ordering::SeqCst);
    let report = library.scan(&root).unwrap();
    assert_eq!(report.errors.len(), 1);
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
}

#[test]
fn cross_instance_index_lock_prevents_scan_and_remove_from_overwriting_each_other() {
    use std::{sync::mpsc, thread};
    struct Gated {
        started: mpsc::SyncSender<()>,
        released: Mutex<mpsc::Receiver<()>>,
        probe: Probe,
    }
    impl AudioProbe for Gated {
        fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
            self.started.send(()).unwrap();
            self.released.lock().unwrap().recv().unwrap();
            self.probe.inspect(path)
        }
    }
    let (_tmp, root, store, _, _) = setup();
    std::fs::write(root.join("a.wav"), "3").unwrap();
    let (send, started) = mpsc::sync_channel(1);
    let (release, recv) = mpsc::channel();
    let library = LocalLibrary::with_probe(
        store.clone(),
        Arc::new(Gated {
            started: send,
            released: Mutex::new(recv),
            probe: Probe::default(),
        }),
    )
    .unwrap();
    let other = LocalLibrary::with_probe(
        Arc::new(StateStore::open(store.directory()).unwrap()),
        Arc::new(Probe::default()),
    )
    .unwrap();
    let worker = thread::spawn(move || library.scan(root));
    started
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(matches!(other.remove("any"), Err(Error::MutationBusy)));
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap().updated, 1);
}

#[test]
fn decoded_ncm_cache_reopen_orphan_and_tamper_repair_preserve_original() {
    let (_tmp, root, store, _, library) = setup();
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ncm.json")).unwrap();
    let original = STANDARD
        .decode(fixture["writer"]["expected_ncm"].as_str().unwrap())
        .unwrap();
    let expected = STANDARD
        .decode(fixture["writer"]["audio"].as_str().unwrap())
        .unwrap();
    let input = root.join("a.ncm");
    std::fs::write(&input, &original).unwrap();
    let output = library.playable_path(&input).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    let reopened = LocalLibrary::new(store.clone()).unwrap();
    assert_eq!(reopened.playable_path(&input).unwrap(), output);
    let records = store.values("local", "decoded_ncm_v1").unwrap();
    let key = records.keys().next().unwrap();
    store.delete("local", "decoded_ncm_v1", key).unwrap();
    assert_eq!(reopened.playable_path(&input).unwrap(), output);
    std::fs::write(&output, b"tampered").unwrap();
    reopened.playable_path(&input).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
fn all_cue_frames_preserve_the_exact_revision_after_sqlite_and_queue_reopen() {
    use cumulus_core::{library::LocalResource, playback::PlaybackQueue};
    let (_tmp, root, store, _, library) = setup();
    std::fs::write(root.join("a.wav"), "3").unwrap();
    let cue = root.join("a.cue");
    let mut queue = PlaybackQueue::new(Some(store.clone()), "local").unwrap();
    for frame in 0..75 {
        std::fs::write(&cue,format!("FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:{frame:02}\nTRACK 02 AUDIO\nINDEX 01 00:02:00\n")).unwrap();
        let (_, tracks) = library.import_cue(&cue).unwrap();
        let resource = library.resource(&tracks[0].id).unwrap();
        assert_eq!(
            resource.local_revision, tracks[0].revision,
            "frame {frame} changed during persistence"
        );
        queue
            .replace(
                vec![serde_json::to_value(&resource).unwrap()],
                0,
                Value::Null,
            )
            .unwrap();
        let restored = PlaybackQueue::new(Some(store.clone()), "local")
            .unwrap()
            .selected()
            .unwrap();
        let restored: LocalResource = serde_json::from_value(restored.resource).unwrap();
        assert_eq!(
            resource.local_revision, restored.local_revision,
            "frame {frame} changed in queue"
        );
        library.resolve(&restored).unwrap();
    }
}
