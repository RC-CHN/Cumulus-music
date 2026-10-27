mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadAction as Action, DownloadDestination as Destination,
            DownloadOptions as Options, DownloadPhase as Phase, DownloadRunOptions as Run,
            DownloadScheduler, DownloadService, DownloadStatus as Status, LyricsStatus,
            OutputContainer,
        },
    },
    media::{
        integrity::inspect_file,
        ncm::{HeaderPolicy, NcmReader},
    },
    membership::Quality,
    storage::StateStore,
};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::{download::setup, transfer::ReadGate};

const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
fn target(root: &Path, name: &str) -> Destination {
    Destination::Path {
        path: root.join(name),
    }
}
fn raw() -> Options {
    Options {
        tags: false,
        cover: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn actual_tags_covers_named_paths_and_lyrics_commit_with_offline_record() {
    for (format, data) in [
        (
            "mp3",
            include_bytes!("fixtures/tag_audio/seed.mp3").as_slice(),
        ),
        ("flac", FLAC),
        (
            "m4a",
            include_bytes!("fixtures/tag_audio/seed.m4a").as_slice(),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (s, session, _, api, b) = setup(root.path(), data.into(), format);
        let task = s
            .create(
                &json!(123),
                Destination::Named {
                    root: root.path().join("out"),
                    path_type: 2,
                    file_type: 0,
                },
                Options {
                    quality: Quality::HiRes,
                    lyrics: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let done = s.run(&task.id, Run::default()).await.unwrap();
        assert_eq!(done.status, Status::Complete);
        assert_eq!(done.phase, Phase::Complete);
        assert_eq!(done.lyrics_status, LyricsStatus::Complete);
        assert!(!done.cleanup_pending);
        let output = done.target.as_ref().unwrap();
        assert!(output.ends_with(format!("Artist/Album/Artist - Title.{format}")));
        assert!(fs::read(output).unwrap() != data);
        assert_eq!(
            fs::read_to_string(output.with_extension("lrc")).unwrap(),
            "[00:00.00]fixture lyrics\n"
        );
        assert!(!task.work.exists());
        let offline = OfflineLibrary::new(session.clone()).unwrap();
        let record = offline
            .record(&session.token().unwrap(), output)
            .unwrap()
            .unwrap();
        assert_eq!(record.quality, Quality::Lossless);
        assert_eq!(record.privilege["flag"], 8196);
        assert!(!record.encrypted);
        assert_eq!(Some(record.integrity), done.output_integrity);
        assert_eq!(b.audio.calls().len(), 1);
        assert_eq!(b.cover.calls().len(), 1);
        assert_eq!(api.count("download/url"), 1);
        assert_eq!(api.count("player/url"), 0);
        assert_eq!(
            s.run(&task.id, Run::default()).await.unwrap().status,
            Status::Complete
        );
        assert_eq!(api.count("download/url"), 1);
        assert!(s.request(&task.id, Action::Cancel).is_err());
        assert!(s.enqueue(&task.id).is_err());
    }
}

#[tokio::test]
async fn pause_reopen_renews_authorization_and_preserves_proven_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, store, api, b) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(&json!(123), target(root.path(), "result.flac"), raw())
        .unwrap();
    let paused = s
        .run(
            &task.id,
            Run {
                byte_budget: Some(123),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(paused.bytes, 123);
    assert!(!root.path().join("result.flac").exists());
    let other = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(store.directory()).unwrap())),
    ));
    other
        .adopt(api.clone(), json!({"userId":1}), &other.token().unwrap())
        .unwrap();
    let reopened = DownloadService::with_backend(other, b.clone(), 1024 * 1024).unwrap();
    let done = reopened.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(done.authorization_count, 2);
    assert_eq!(b.audio.calls()[1].offset, 123);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), FLAC);
}

#[tokio::test]
async fn authorization_denials_trial_identity_and_bad_domains_stop_before_media() {
    for change in [
        json!({"code":403}),
        json!({"id":999}),
        json!({"freeTrialInfo":{}}),
        json!({"url":"https://unrelated.example/file"}),
        json!({"size":0}),
        json!({"level":null}),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (s, _, _, api, b) = setup(root.path(), FLAC.into(), "flac");
        api.resource
            .lock()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let task = s
            .create(
                &json!(123),
                target(root.path(), "result.flac"),
                Options::default(),
            )
            .unwrap();
        assert!(s.run(&task.id, Run::default()).await.is_err());
        assert_eq!(s.get(&task.id).unwrap().status, Status::Error);
        assert_eq!(api.count("download/url"), 1);
        assert!(b.audio.calls().is_empty());
        assert!(!root.path().join("result.flac").exists());
        s.request(&task.id, Action::Cancel).unwrap();
        assert!(!task.work.exists());
    }
}

#[tokio::test]
async fn cdn_denial_has_exactly_one_renewal_and_no_play_url_fallback() {
    for failures in [1, 2] {
        let root = tempfile::tempdir().unwrap();
        let (s, _, _, api, b) = setup(root.path(), FLAC.into(), "flac");
        b.http_failures.store(failures, Ordering::SeqCst);
        let task = s
            .create(&json!(123), target(root.path(), "result.flac"), raw())
            .unwrap();
        let result = s.run(&task.id, Run::default()).await;
        assert_eq!(result.is_ok(), failures == 1);
        assert_eq!(api.count("download/url"), 2);
        assert_eq!(api.count("player/url"), 0);
        if failures == 2 {
            assert_eq!(s.get(&task.id).unwrap().service_code, Some(json!(403)));
        }
    }
}

#[tokio::test]
async fn cover_failure_reuses_verified_raw_media_and_raw_mode_skips_postprocessing() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, api, b) = setup(root.path(), FLAC.into(), "flac");
    b.cover.settings.lock().unwrap().data = b"broken cover".to_vec();
    let task = s
        .create(
            &json!(123),
            target(root.path(), "result.flac"),
            Options::default(),
        )
        .unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    assert_eq!(s.get(&task.id).unwrap().phase, Phase::Prepare);
    assert!(!root.path().join("result.flac").exists());
    b.cover.settings.lock().unwrap().data = include_bytes!("fixtures/tag_audio/cover.png").to_vec();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(api.count("download/url"), 1);
    assert_eq!(b.audio.calls().len(), 1);
    assert_eq!(b.cover.calls().len(), 2);
}

#[tokio::test]
async fn final_database_failure_rolls_back_ledger_and_recovers_without_network_or_retagging() {
    let root = tempfile::tempdir().unwrap();
    let (s, session, store, api, b) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(
            &json!(123),
            target(root.path(), "result.flac"),
            Options::default(),
        )
        .unwrap();
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(matches!(
        s.run(&task.id, Run::default()).await,
        Err(Error::Storage(_))
    ));
    let old = s.get(&task.id).unwrap();
    assert_eq!(old.phase, Phase::Publish);
    assert_eq!(old.status, Status::Error);
    let output = old.target.as_ref().unwrap();
    let digest = inspect_file(output).unwrap();
    assert!(
        OfflineLibrary::new(session.clone())
            .unwrap()
            .records(&session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(s.request(&task.id, Action::Cancel).is_err());
    assert!(s.restart_transfer(&task.id).is_err());
    db.execute_batch("DROP TRIGGER fail_complete;").unwrap();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(inspect_file(output).unwrap(), digest);
    assert_eq!(api.count("download/url"), 1);
    assert_eq!(b.audio.calls().len(), 1);
    assert_eq!(b.cover.calls().len(), 1);
    assert!(!task.work.exists());
}

#[tokio::test]
async fn foreign_destination_is_preserved_after_publication_failure() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, store, _, _) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(&json!(123), target(root.path(), "result.flac"), raw())
        .unwrap();
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    db.execute_batch("DROP TRIGGER fail_complete;").unwrap();
    let output = root.path().join("result.flac");
    fs::remove_file(&output).unwrap();
    fs::write(&output, FLAC).unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    assert_eq!(
        s.request(&task.id, Action::Cancel).unwrap().status,
        Status::Cancelled
    );
    assert_eq!(fs::read(&output).unwrap(), FLAC);
}

#[tokio::test]
async fn durable_pause_cancel_and_account_switch_are_observed_after_pending_read() {
    for action in [Some(Action::Pause), Some(Action::Cancel), None] {
        let root = tempfile::tempdir().unwrap();
        let (s, session, _, api, b) = setup(root.path(), FLAC.into(), "flac");
        let task = s
            .create(&json!(123), target(root.path(), "result.flac"), raw())
            .unwrap();
        let gate = Arc::new(ReadGate::default());
        b.audio.settings.lock().unwrap().gate = Some(gate.clone());
        let worker = {
            let s = s.clone();
            let id = task.id.clone();
            tokio::spawn(async move { s.run(&id, Run::default()).await })
        };
        tokio::time::timeout(Duration::from_secs(3), gate.reached.notified())
            .await
            .unwrap();
        assert!(matches!(
            s.run(&task.id, Run::default()).await,
            Err(Error::MutationBusy)
        ));
        if let Some(a) = action {
            s.request(&task.id, a).unwrap();
        } else {
            session
                .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
                .unwrap();
        }
        gate.release.notify_one();
        let result = worker.await.unwrap();
        assert!(!root.path().join("result.flac").exists());
        match action {
            Some(Action::Pause) => {
                assert_eq!(result.unwrap().status, Status::Paused);
                assert_eq!(
                    fs::metadata(task.work.join(format!(".cumulus-{}.part", task.id)))
                        .unwrap()
                        .len(),
                    0
                );
                b.audio.settings.lock().unwrap().gate = None;
                s.enqueue(&task.id).unwrap();
                assert_eq!(
                    s.run(&task.id, Run::default()).await.unwrap().status,
                    Status::Complete
                );
            }
            Some(Action::Cancel) => {
                assert_eq!(result.unwrap().status, Status::Cancelled);
                assert!(!task.work.exists());
            }
            None => {
                assert!(matches!(result, Err(Error::StaleResponse)));
                assert!(s.get(&task.id).is_err());
            }
        }
    }
}

#[tokio::test]
async fn ancillary_lyric_failure_and_conflict_never_downgrade_media_completion() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, api, b) = setup(root.path(), FLAC.into(), "flac");
    *api.lyric.lock().unwrap() = Err(Error::Response("fixture"));
    let task = s
        .create(
            &json!(123),
            target(root.path(), "result.flac"),
            Options {
                lyrics: true,
                ..raw()
            },
        )
        .unwrap();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(done.lyrics_status, LyricsStatus::Error);
    assert!(done.cleanup_pending);
    assert!(task.work.exists());
    *api.lyric.lock().unwrap() = Ok(json!({"code":200,"lrc":{"lyric":"[00:00.00]new"}}));
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.lyrics_status, LyricsStatus::Complete);
    assert!(!done.cleanup_pending);
    assert_eq!(b.audio.calls().len(), 1);
    assert_eq!(api.count("download/url"), 1);
    let second = s
        .create(
            &json!(123),
            target(root.path(), "second.flac"),
            Options {
                lyrics: true,
                ..raw()
            },
        )
        .unwrap();
    fs::write(root.path().join("second.lrc"), b"keep").unwrap();
    let done = s.run(&second.id, Run::default()).await.unwrap();
    assert_eq!(done.lyrics_status, LyricsStatus::Conflict);
    assert_eq!(fs::read(root.path().join("second.lrc")).unwrap(), b"keep");
}

#[tokio::test]
async fn optional_ncm_uses_actual_quality_preserves_audio_and_registers_entitlements() {
    let root = tempfile::tempdir().unwrap();
    let (s, session, _, api, _) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(
            &json!(123),
            target(root.path(), "result.flac"),
            Options {
                container: OutputContainer::Official,
                quality: Quality::HiRes,
                ..Default::default()
            },
        )
        .unwrap();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert!(done.encrypted && done.official_encryption);
    let path = done.target.unwrap();
    assert_eq!(path.extension().unwrap(), "ncm");
    let mut ncm = NcmReader::open(&path, HeaderPolicy::Strict).unwrap();
    assert!(ncm.info().crc_verified);
    assert!(ncm.info().cover_bytes > 0);
    let mut data = Vec::new();
    ncm.read_to_end(&mut data).unwrap();
    assert_eq!(data, FLAC);
    let check = OfflineLibrary::new(session.clone())
        .unwrap()
        .check(&session.token().unwrap(), &path, false)
        .await
        .unwrap();
    assert_eq!(
        check.access.reason,
        Some(cumulus_core::membership::OfflineDenial::Album)
    );
    assert!(!check.integrity_verified);
    assert_eq!(check.record.quality, Quality::Lossless);
    assert_eq!(api.count("vip/info"), 1);
    // A different ordinary vinyl resource is allowed; do not discard album
    // privilege bits just to make the first resource pass.
    api.song.lock().unwrap()["privilege"]["flag"] = json!(4);
    let ordinary = s
        .create(
            &json!(123),
            target(root.path(), "ordinary.flac"),
            Options {
                container: OutputContainer::Ncm,
                ..Default::default()
            },
        )
        .unwrap();
    let done = s.run(&ordinary.id, Run::default()).await.unwrap();
    let check = OfflineLibrary::new(session.clone())
        .unwrap()
        .check(&session.token().unwrap(), done.target.unwrap(), false)
        .await
        .unwrap();
    assert!(check.access.can_play && check.integrity_verified);
}

#[tokio::test]
async fn scheduler_limits_full_tasks_stops_active_workers_and_keeps_waiting_tasks_queued() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, _, b) = setup(root.path(), FLAC.into(), "flac");
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(
            s.create(
                &json!(123),
                target(root.path(), &format!("{i}.flac")),
                raw(),
            )
            .unwrap()
            .id,
        );
    }
    let gate = Arc::new(ReadGate::default());
    b.audio.settings.lock().unwrap().gate = Some(gate.clone());
    let scheduler = Arc::new(DownloadScheduler::new(s.clone(), 3).unwrap());
    let worker = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.run_pending().await })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        while b.audio.calls().len() < 3 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(b.audio.calls().len(), 3);
    assert!(matches!(
        scheduler.run_pending().await,
        Err(Error::MutationBusy)
    ));
    scheduler.stop();
    gate.release.notify_waiters();
    let result = worker.await.unwrap().unwrap();
    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|r| r.status == Status::Paused));
    let queued = s
        .list()
        .unwrap()
        .into_iter()
        .filter(|t| t.status == Status::Queued)
        .count();
    assert_eq!(queued, 1);
    b.audio.settings.lock().unwrap().gate = None;
    for id in &ids {
        s.enqueue(id).unwrap();
    }
    let result = scheduler.run_pending().await.unwrap();
    assert_eq!(result.len(), 4);
    assert!(result.iter().all(|r| r.status == Status::Complete));
}
