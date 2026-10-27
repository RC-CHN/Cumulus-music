mod support;
use cumulus_core::{
    Error,
    download::transfer::{
        CreateTransfer, MediaUrl, RunOptions, TransferDirective, TransferJob, TransferManager,
        TransferResource, TransferStatus as Status,
    },
    storage::StateStore,
};
use md5::{Digest, Md5};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use support::transfer::{Origin, ReadGate};

fn resource(data: &[u8]) -> TransferResource {
    TransferResource {
        url: MediaUrl::parse("http://m.test.music.126.net/media?token=synthetic").unwrap(),
        size: Some(data.len() as u64),
        md5: Some(hex::encode(Md5::digest(data))),
    }
}
fn manager(store: Arc<StateStore>, origin: Arc<Origin>) -> TransferManager {
    TransferManager::with_backend(store, "1".into(), origin, 512 * 1024 * 1024).unwrap()
}
fn create(manager: &TransferManager, root: &Path, data: &[u8]) -> TransferJob {
    manager
        .create(CreateTransfer {
            resource: resource(data),
            target: root.join("audio.flac"),
            resource_id: Some("123".into()),
            id: None,
        })
        .unwrap()
}
fn budget(bytes: u64) -> RunOptions {
    RunOptions {
        byte_budget: Some(bytes),
        ..Default::default()
    }
}

#[test]
fn media_url_is_redacted_and_cdn_authority_is_strict() {
    let valid =
        MediaUrl::parse("http://vodkgeyttp8.vod.126.net/video.mp4?token=synthetic").unwrap();
    assert_eq!(
        valid.as_str(),
        "https://vodkgeyttp8.vod.126.net/video.mp4?token=synthetic"
    );
    assert!(!format!("{valid:?}").contains("synthetic"));
    for url in [
        "https://music.126.net.attacker.test/a",
        "https://a.vod.126.net@attacker.test/a",
        "https://a.vod.126.net:8443/a",
        "https://a.vod.126.net/a#",
        "https://unrelated.126.net/a",
        "http://127.0.0.1/a",
        "file:///tmp/a",
        "https://user:pass@m.test.music.126.net/a",
        "https://a.music.126.net/\r\nx",
        "https://a.music.126.net\\@evil.test/a",
    ] {
        assert!(MediaUrl::parse(url).is_err(), "{url:?}");
    }
}

#[tokio::test]
async fn resume_after_store_reopen_uses_actual_bytes_and_if_range() {
    let root = tempfile::tempdir().unwrap();
    let data = (0..256)
        .cycle()
        .take(256000)
        .map(|n| n as u8)
        .collect::<Vec<_>>();
    let state = root.path().join("state");
    let store = Arc::new(StateStore::open(&state).unwrap());
    let origin = Arc::new(Origin::new(data.clone()));
    let m = manager(store, origin.clone());
    let job = create(&m, root.path(), &data);
    let paused = m.run(&job.id, budget(65537)).await.unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(paused.bytes, 65537);
    assert!(!job.target.exists());
    let other = manager(Arc::new(StateStore::open(state).unwrap()), origin.clone());
    let done = other.run(&job.id, RunOptions::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(done.bytes, data.len() as u64);
    assert!(!done.cleanup_pending);
    assert!(!job.part.exists());
    assert_eq!(fs::read(&job.target).unwrap(), data);
    assert_eq!(origin.calls()[1].offset, 65537);
    assert_eq!(
        origin.calls()[1].if_range.as_deref(),
        Some("\"fixture-v1\"")
    );
    assert_eq!(
        other
            .run(&job.id, RunOptions::default())
            .await
            .unwrap()
            .status,
        Status::Complete
    );
    assert_eq!(origin.calls().len(), 2);
    assert!(
        TransferManager::new(
            Arc::new(StateStore::open(root.path().join("state")).unwrap()),
            "2".into()
        )
        .unwrap()
        .get(&job.id)
        .is_err()
    );
}

#[tokio::test]
async fn valid_short_ranges_pause_and_ignored_range_replaces_partial_bytes() {
    let root = tempfile::tempdir().unwrap();
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    origin.settings.lock().unwrap().range_limit = Some(123);
    let m = manager(
        Arc::new(StateStore::open(root.path().join("state")).unwrap()),
        origin.clone(),
    );
    let job = create(&m, root.path(), &data);
    let paused = m.run(&job.id, RunOptions::default()).await.unwrap();
    assert_eq!(paused.bytes, 123);
    assert_eq!(paused.status, Status::Paused);
    {
        let mut s = origin.settings.lock().unwrap();
        s.range_limit = None;
        s.ignore_range = true;
    }
    assert_eq!(
        m.run(&job.id, RunOptions::default()).await.unwrap().status,
        Status::Complete
    );
    assert_eq!(fs::read(job.target).unwrap(), data);
}

#[tokio::test]
async fn bad_ranges_encoding_length_and_changed_validators_never_publish() {
    for mode in ["range", "etag", "encoding", "length", "http"] {
        let root = tempfile::tempdir().unwrap();
        let data = b"audio".repeat(1000);
        let origin = Arc::new(Origin::new(data.clone()));
        let m = manager(
            Arc::new(StateStore::open(root.path().join("state")).unwrap()),
            origin.clone(),
        );
        let job = create(&m, root.path(), &data);
        m.run(&job.id, budget(123)).await.unwrap();
        {
            let mut s = origin.settings.lock().unwrap();
            match mode {
                "range" => s.corrupt_range = true,
                "etag" => s.etag = Some("\"changed\"".into()),
                "encoding" => s.encoding = Some("gzip".into()),
                "length" => s.content_length = Some("-1".into()),
                _ => {
                    s.status = Some(403);
                    s.encoding = Some("gzip".into());
                }
            }
        }
        let result = m.run(&job.id, RunOptions::default()).await;
        if mode == "http" {
            assert!(matches!(result, Err(Error::HttpStatus { status: 403, .. })));
        } else {
            assert!(result.is_err());
        }
        assert!(!job.target.exists());
        let saved = m.get(&job.id).unwrap();
        assert_eq!(saved.status, Status::Error);
        assert_eq!(saved.bytes, 123);
    }
}

#[tokio::test]
async fn truncated_and_corrupt_media_require_verified_resume_or_explicit_restart() {
    let root = tempfile::tempdir().unwrap();
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    origin.settings.lock().unwrap().truncate = 3;
    let m = manager(
        Arc::new(StateStore::open(root.path().join("state")).unwrap()),
        origin.clone(),
    );
    let job = create(&m, root.path(), &data);
    assert!(m.run(&job.id, RunOptions::default()).await.is_err());
    assert_eq!(m.get(&job.id).unwrap().bytes, 4997);
    origin.settings.lock().unwrap().truncate = 0;
    assert_eq!(
        m.run(&job.id, RunOptions::default()).await.unwrap().status,
        Status::Complete
    );
    let second = root.path().join("second");
    fs::create_dir(&second).unwrap();
    origin.settings.lock().unwrap().data = b"bad!!".repeat(1000);
    let job = create(&m, &second, &data);
    assert!(matches!(
        m.run(&job.id, RunOptions::default()).await,
        Err(Error::Media(_))
    ));
    assert!(!job.target.exists());
    let requests = origin.calls().len();
    assert!(m.run(&job.id, RunOptions::default()).await.is_err());
    assert_eq!(origin.calls().len(), requests);
    m.restart(&job.id).unwrap();
    origin.settings.lock().unwrap().data = data.clone();
    m.run(&job.id, RunOptions::default()).await.unwrap();
    assert_eq!(fs::read(job.target).unwrap(), data);
}

#[tokio::test]
async fn renewal_preserves_only_proven_representation_and_unknown_validators_restart() {
    let root = tempfile::tempdir().unwrap();
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    let m = manager(
        Arc::new(StateStore::open(root.path().join("state")).unwrap()),
        origin.clone(),
    );
    let job = create(&m, root.path(), &data);
    m.run(&job.id, budget(123)).await.unwrap();
    let same = m.refresh_resource(&job.id, resource(&data)).unwrap();
    assert!(!same.representation_restarted);
    assert!(same.etag.is_none());
    m.run(&job.id, budget(7)).await.unwrap();
    assert_eq!(origin.calls()[1].offset, 123);
    assert!(origin.calls()[1].if_range.is_none());
    let replacement = b"other".repeat(900);
    origin.settings.lock().unwrap().data = replacement.clone();
    assert!(
        m.refresh_resource(&job.id, resource(&replacement))
            .unwrap()
            .representation_restarted
    );
    assert_eq!(fs::metadata(&job.part).unwrap().len(), 0);
    m.run(&job.id, RunOptions::default()).await.unwrap();
    assert_eq!(fs::read(job.target).unwrap(), replacement);
    let dir = root.path().join("unknown");
    fs::create_dir(&dir).unwrap();
    {
        let mut s = origin.settings.lock().unwrap();
        s.data = data.clone();
        s.etag = Some("W/\"weak\"".into());
    }
    let mut r = resource(&data);
    r.md5 = None;
    let job = m
        .create(CreateTransfer {
            resource: r,
            target: dir.join("unknown"),
            resource_id: None,
            id: None,
        })
        .unwrap();
    m.run(&job.id, budget(23)).await.unwrap();
    m.run(&job.id, RunOptions::default()).await.unwrap();
    assert_eq!(origin.calls().last().unwrap().offset, 0);
    assert_eq!(fs::read(job.target).unwrap(), data);
}

#[tokio::test]
async fn publication_commit_recovery_never_truncates_or_adopts_an_unrelated_target() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    let m = manager(Arc::new(StateStore::open(&state).unwrap()), origin.clone());
    let job = create(&m, root.path(), &data);
    let db = rusqlite::Connection::open(state.join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON state WHEN NEW.kind='media_transfers_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(matches!(
        m.run(&job.id, RunOptions::default()).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(m.get(&job.id).unwrap().status, Status::Publishing);
    assert_eq!(fs::read(&job.target).unwrap(), data);
    assert!(same_file::is_same_file(&job.part, &job.target).unwrap());
    assert!(m.restart(&job.id).is_err());
    assert!(m.refresh_resource(&job.id, resource(&data)).is_err());
    assert!(m.cancel(&job.id).is_err());
    assert_eq!(fs::read(&job.target).unwrap(), data);
    db.execute_batch("DROP TRIGGER fail_complete;").unwrap();
    let reopened = manager(Arc::new(StateStore::open(state).unwrap()), origin.clone());
    assert_eq!(
        reopened
            .run(&job.id, RunOptions::default())
            .await
            .unwrap()
            .status,
        Status::Complete
    );
    assert_eq!(origin.calls().len(), 1);
    let dir = root.path().join("foreign");
    fs::create_dir(&dir).unwrap();
    let second = create(&m, &dir, &data);
    fs::write(&second.target, &data).unwrap();
    assert!(m.run(&second.id, RunOptions::default()).await.is_err());
    assert_eq!(fs::read(&second.target).unwrap(), data);
    m.cancel(&second.id).unwrap();
    assert!(second.target.exists());
    assert!(!second.part.exists());
}

#[tokio::test]
async fn abort_and_account_control_release_locks_without_writing_late_body() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    let gate = Arc::new(ReadGate::default());
    origin.settings.lock().unwrap().gate = Some(gate.clone());
    let m = Arc::new(manager(
        Arc::new(StateStore::open(&state).unwrap()),
        origin.clone(),
    ));
    let job = create(&m, root.path(), &data);
    let worker = {
        let m = m.clone();
        let id = job.id.clone();
        tokio::spawn(async move { m.run(&id, RunOptions::default()).await })
    };
    gate.reached.notified().await;
    let other = manager(Arc::new(StateStore::open(&state).unwrap()), origin.clone());
    assert!(matches!(other.restart(&job.id), Err(Error::MutationBusy)));
    worker.abort();
    assert!(matches!(worker.await,Err(error) if error.is_cancelled()));
    assert_eq!(other.get(&job.id).unwrap().status, Status::Paused);
    let stale = Arc::new(AtomicBool::new(false));
    let worker = {
        let m = m.clone();
        let id = job.id.clone();
        let stale = stale.clone();
        tokio::spawn(async move {
            m.run(
                &id,
                RunOptions {
                    byte_budget: None,
                    control: Arc::new(move || {
                        if stale.load(Ordering::SeqCst) {
                            Err(Error::StaleResponse)
                        } else {
                            Ok(TransferDirective::Continue)
                        }
                    }),
                },
            )
            .await
        })
    };
    gate.reached.notified().await;
    stale.store(true, Ordering::SeqCst);
    gate.release.notify_one();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(other.get(&job.id).unwrap().bytes, 0);
    origin.settings.lock().unwrap().gate = None;
    assert_eq!(
        other
            .run(&job.id, RunOptions::default())
            .await
            .unwrap()
            .status,
        Status::Complete
    );
}

#[tokio::test]
async fn failed_journal_and_replaced_staging_are_not_network_or_deletion_success() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let data = b"audio".repeat(1000);
    let origin = Arc::new(Origin::new(data.clone()));
    let m = manager(Arc::new(StateStore::open(&state).unwrap()), origin.clone());
    let db = rusqlite::Connection::open(state.join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_create BEFORE INSERT ON state WHEN NEW.kind='media_transfers_v1' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(
        m.create(CreateTransfer {
            resource: resource(&data),
            target: root.path().join("failed"),
            resource_id: None,
            id: None
        })
        .is_err()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    db.execute_batch("DROP TRIGGER fail_create;").unwrap();
    let job = create(&m, root.path(), &data);
    db.execute_batch("CREATE TRIGGER fail_run BEFORE INSERT ON state WHEN NEW.kind='media_transfers_v1' AND json_extract(NEW.value,'$.status')='running' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(m.run(&job.id, RunOptions::default()).await.is_err());
    assert!(origin.calls().is_empty());
    db.execute_batch("DROP TRIGGER fail_run;").unwrap();
    let foreign = root.path().join("foreign");
    fs::write(&foreign, b"my file").unwrap();
    fs::rename(&foreign, &job.part).unwrap();
    assert!(m.cancel(&job.id).is_err());
    assert_eq!(fs::read(&job.part).unwrap(), b"my file");
    assert!(!job.target.exists());
}
