use cumulus_core::{
    Error, Result,
    library::{
        AudioMetadata, ListOptions, LocalLibrary,
        monitor::{LibraryMonitor, WatchStatus},
        probe::{AudioProbe, parse_ffprobe_response},
    },
    storage::StateStore,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        let duration = std::fs::read_to_string(path)?
            .trim()
            .parse::<f64>()
            .map_err(|_| Error::Media("synthetic audio invalid"))?;
        parse_ffprobe_response(
            path,
            &json!({"streams":[{"codec_type":"audio","codec_name":"wav"}],"format":{"duration":duration}}),
        )
    }
}
fn setup() -> (
    tempfile::TempDir,
    PathBuf,
    Arc<StateStore>,
    LocalLibrary,
    LibraryMonitor,
) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let library = LocalLibrary::with_probe(store.clone(), Arc::new(Probe)).unwrap();
    let monitor = LibraryMonitor::new(library.clone(), Duration::from_millis(50)).unwrap();
    (tmp, root, store, library, monitor)
}

#[test]
fn durable_roots_missing_recovery_and_wrong_type_preserve_consistent_indexes() {
    let (tmp, root, store, library, monitor) = setup();
    let audio = root.join("a.wav");
    std::fs::write(&audio, "3").unwrap();
    let watched = monitor.watch(&root).unwrap();
    assert_eq!(watched.revision, 1);
    let result = monitor.run_once().unwrap();
    assert_eq!(result[0].status, WatchStatus::Ready);
    assert_eq!(result[0].result.as_ref().unwrap().updated, 1);
    let reopened = LibraryMonitor::new(
        LocalLibrary::with_probe(
            Arc::new(StateStore::open(store.directory()).unwrap()),
            Arc::new(Probe),
        )
        .unwrap(),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(reopened.roots().unwrap()[0].id, watched.id);
    let moved = tmp.path().join("old-root");
    std::fs::rename(&root, &moved).unwrap();
    std::fs::write(&root, "not a directory").unwrap();
    let result = reopened.run_once().unwrap();
    assert_eq!(result[0].status, WatchStatus::Error);
    assert!(
        !library
            .list(ListOptions {
                include_missing: true,
                ..Default::default()
            })
            .unwrap()[0]
            .missing
    );
    std::fs::remove_file(&root).unwrap();
    let result = reopened.run_once().unwrap();
    let report = result[0].result.as_ref().unwrap();
    assert!(report.root_missing);
    assert_eq!(report.missing, 1);
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
    std::fs::rename(moved, &root).unwrap();
    let result = reopened.run_once().unwrap();
    assert_eq!(result[0].status, WatchStatus::Ready);
    assert_eq!(library.list(ListOptions::default()).unwrap().len(), 1);
    reopened.unwatch(&watched.id).unwrap();
    assert!(audio.exists());
    assert!(reopened.run_once().unwrap().is_empty());
    assert_eq!(library.list(ListOptions::default()).unwrap().len(), 1);
}

#[test]
fn unwatch_during_probe_cancels_publication_and_late_completion_cannot_reenable_root() {
    struct Gated {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl AudioProbe for Gated {
        fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Probe.inspect(path)
        }
    }
    let (_tmp, root, store, _, _) = setup();
    std::fs::write(root.join("a.wav"), "3").unwrap();
    let (send, entered) = mpsc::sync_channel(1);
    let (release, receive) = mpsc::channel();
    let library = LocalLibrary::with_probe(
        store.clone(),
        Arc::new(Gated {
            entered: send,
            release: Mutex::new(receive),
        }),
    )
    .unwrap();
    let monitor = Arc::new(LibraryMonitor::new(library.clone(), Duration::from_secs(2)).unwrap());
    let root_state = monitor.watch(&root).unwrap();
    let other = LibraryMonitor::new(
        LocalLibrary::with_probe(
            Arc::new(StateStore::open(store.directory()).unwrap()),
            Arc::new(Probe),
        )
        .unwrap(),
        Duration::from_secs(2),
    )
    .unwrap();
    let worker = thread::spawn({
        let monitor = monitor.clone();
        move || monitor.run_once()
    });
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(other.run_once().unwrap().is_empty());
    let disabled = other.unwatch(&root_state.id).unwrap().unwrap();
    assert_eq!(disabled.revision, 2);
    release.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap().is_empty());
    assert_eq!(monitor.roots().unwrap()[0].status, WatchStatus::Disabled);
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
    assert!(root.join("a.wav").exists());
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "monitor did not converge"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn background_polling_start_stop_restart_and_bounded_events_converge() {
    let (_tmp, root, _, library, monitor) = setup();
    let receiver = monitor.subscribe().unwrap();
    let mut watched = monitor.watch(&root).unwrap();
    for _ in 0..40 {
        watched = monitor.watch(&root).unwrap();
    }
    assert_eq!(receiver.try_iter().count(), 32);
    monitor.start().unwrap();
    monitor.start().unwrap();
    std::fs::write(root.join("a.wav"), "3").unwrap();
    wait_until(|| library.list(ListOptions::default()).unwrap().len() == 1);
    std::fs::write(root.join("a.wav"), "4 ").unwrap();
    wait_until(|| {
        library.list(ListOptions::default()).unwrap()[0]
            .metadata
            .duration
            == 4.0
    });
    std::fs::remove_file(root.join("a.wav")).unwrap();
    wait_until(|| library.list(ListOptions::default()).unwrap().is_empty());
    monitor.stop().unwrap();
    std::fs::write(root.join("a.wav"), "5  ").unwrap();
    assert!(monitor.run_once().unwrap().is_empty());
    monitor.start().unwrap();
    wait_until(|| {
        library
            .list(ListOptions::default())
            .unwrap()
            .first()
            .is_some_and(|i| i.metadata.duration == 5.0)
    });
    monitor.unwatch(&watched.id).unwrap();
    monitor.stop().unwrap();
    assert!(root.join("a.wav").exists());
    monitor.start().unwrap();
    thread::sleep(Duration::from_millis(60));
    assert_eq!(monitor.roots().unwrap()[0].status, WatchStatus::Disabled);
    monitor.stop().unwrap();
}

#[test]
fn root_registration_is_transactional_and_failed_scan_can_be_reconciled() {
    let (_tmp, root, store, library, monitor) = setup();
    assert!(LibraryMonitor::new(library.clone(), Duration::from_millis(1)).is_err());
    assert!(monitor.watch(store.directory()).is_err());
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_watch BEFORE INSERT ON state WHEN NEW.kind='library_watch_roots_v1' BEGIN SELECT RAISE(FAIL,'test failure'); END;").unwrap();
    assert!(matches!(monitor.watch(&root), Err(Error::Storage(_))));
    assert!(monitor.roots().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_watch;").unwrap();
    let root_state = monitor.watch(&root).unwrap();
    std::fs::write(root.join("a.wav"), "3").unwrap();
    db.execute_batch("CREATE TRIGGER fail_index BEFORE INSERT ON state WHEN NEW.kind='library_files_v1' BEGIN SELECT RAISE(FAIL,'test failure'); END;").unwrap();
    assert_eq!(monitor.run_once().unwrap()[0].status, WatchStatus::Error);
    assert!(library.list(ListOptions::default()).unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_index;").unwrap();
    let result = monitor.run_once().unwrap();
    assert_eq!(result[0].status, WatchStatus::Ready);
    assert_eq!(result[0].id, root_state.id);
    assert_eq!(library.list(ListOptions::default()).unwrap().len(), 1);
}
