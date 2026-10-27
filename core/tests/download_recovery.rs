mod support;
use cumulus_core::{
    Error,
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadAction as Action, DownloadDestination as Destination,
            DownloadOptions as Options, DownloadPhase as Phase, DownloadRunOptions as Run,
            DownloadScheduler, DownloadStatus as Status, LyricsStatus,
        },
    },
    media::integrity::inspect_file,
};
use serde_json::json;
use std::{fs, sync::Arc, time::Duration};
use support::{download::setup, transfer::ReadGate};
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
fn options() -> Options {
    Options {
        tags: false,
        cover: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn journal_failures_precede_requests_and_reserved_files_are_cleaned() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, store, api, b) = setup(root.path(), FLAC.into(), "flac");
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_create BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        s.create(
            &json!(123),
            Destination::Path {
                path: root.path().join("fail.flac")
            },
            options()
        )
        .is_err()
    );
    assert!(fs::read_dir(root.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".cumulus-")
    }));
    assert!(api.calls.lock().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_create;").unwrap();
    let task = s
        .create(
            &json!(123),
            Destination::Path {
                path: root.path().join("ok.flac"),
            },
            options(),
        )
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_auth BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='authorizing' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(matches!(
        s.run(&task.id, Run::default()).await,
        Err(Error::Storage(_))
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    assert!(b.audio.calls().is_empty());
    db.execute_batch("DROP TRIGGER fail_auth;").unwrap();
    db.execute_batch("CREATE TRIGGER fail_artifact BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.prepared.path') IS NOT NULL BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(matches!(
        s.run(&task.id, Run::default()).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(s.get(&task.id).unwrap().phase, Phase::Prepare);
    assert_eq!(fs::read_dir(&task.work).unwrap().count(), 1);
    assert!(task.work.join("raw").exists());
    db.execute_batch("DROP TRIGGER fail_artifact;").unwrap();
    assert_eq!(
        s.run(&task.id, Run::default()).await.unwrap().status,
        Status::Complete
    );
    assert_eq!(b.audio.calls().len(), 1);
}

#[tokio::test]
async fn future_abort_releases_worker_and_resumes_existing_task_with_fresh_authorization() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, api, b) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(
            &json!(123),
            Destination::Path {
                path: root.path().join("ok.flac"),
            },
            options(),
        )
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
    worker.abort();
    assert!(matches!(worker.await,Err(e) if e.is_cancelled()));
    b.audio.settings.lock().unwrap().gate = None;
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(api.count("download/url"), 2);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), FLAC);
}

#[tokio::test]
async fn changed_prepared_file_is_not_published_or_deleted_as_owned() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, store, _, b) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(
            &json!(123),
            Destination::Path {
                path: root.path().join("ok.flac"),
            },
            options(),
        )
        .unwrap();
    // Deny publication at the target directory without losing the prepared
    // journal: trigger final commit first, then remove our hard link.
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    db.execute_batch("DROP TRIGGER fail_complete;").unwrap();
    fs::remove_file(root.path().join("ok.flac")).unwrap();
    let saved = store
        .get("1", "download_tasks_v1", &task.id)
        .unwrap()
        .unwrap();
    let artifact = std::path::PathBuf::from(saved["prepared"]["path"].as_str().unwrap());
    fs::rename(&artifact, task.work.join("original-kept")).unwrap();
    fs::write(&artifact, b"foreign").unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    assert_eq!(b.audio.calls().len(), 1);
    assert!(!root.path().join("ok.flac").exists());
    let cancelled = s.request(&task.id, Action::Cancel).unwrap();
    assert_eq!(cancelled.status, Status::Cancelled);
    assert!(cancelled.cleanup_pending);
    assert_eq!(fs::read(artifact).unwrap(), b"foreign");
    assert!(task.work.join("original-kept").exists());
}

#[tokio::test]
async fn lyric_link_commit_failure_recovers_sidecar_without_second_lyric_request() {
    let root = tempfile::tempdir().unwrap();
    let (s, session, store, api, b) = setup(root.path(), FLAC.into(), "flac");
    let task = s
        .create(
            &json!(123),
            Destination::Path {
                path: root.path().join("ok.flac"),
            },
            Options {
                lyrics: true,
                ..options()
            },
        )
        .unwrap();
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_lyric BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.lyrics_status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(done.lyrics_status, LyricsStatus::Error);
    assert!(root.path().join("ok.lrc").exists());
    assert_eq!(
        OfflineLibrary::new(session.clone())
            .unwrap()
            .records(&session.token().unwrap())
            .unwrap()
            .len(),
        1
    );
    let before = inspect_file(root.path().join("ok.lrc")).unwrap();
    db.execute_batch("DROP TRIGGER fail_lyric;").unwrap();
    let done = s.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.lyrics_status, LyricsStatus::Complete);
    assert!(!done.cleanup_pending);
    assert_eq!(inspect_file(root.path().join("ok.lrc")).unwrap(), before);
    assert_eq!(api.count("lyric/v1"), 1);
    assert_eq!(b.audio.calls().len(), 1);
}

#[tokio::test]
async fn selected_queue_preserves_unselected_tasks_and_isolates_failure() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, _, b) = setup(root.path(), FLAC.into(), "flac");
    let mut tasks = Vec::new();
    for name in ["bad", "good", "waiting"] {
        tasks.push(
            s.create(
                &json!(123),
                Destination::Path {
                    path: root.path().join(format!("{name}.flac")),
                },
                options(),
            )
            .unwrap(),
        );
    }
    fs::write(root.path().join("bad.flac"), b"keep").unwrap();
    let ids = tasks[..2].iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    let results = DownloadScheduler::new(s.clone(), 2)
        .unwrap()
        .run_selected(&ids)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(s.get(&tasks[0].id).unwrap().status, Status::Error);
    assert_eq!(s.get(&tasks[1].id).unwrap().status, Status::Complete);
    assert_eq!(s.get(&tasks[2].id).unwrap().status, Status::Queued);
    assert_eq!(b.audio.calls().len(), 1);
    assert_eq!(fs::read(root.path().join("bad.flac")).unwrap(), b"keep");
}

#[cfg(unix)]
#[tokio::test]
async fn named_layout_refuses_symlink_directory_redirection() {
    let root = tempfile::tempdir().unwrap();
    let (s, _, _, _, b) = setup(root.path(), FLAC.into(), "flac");
    let output = root.path().join("output");
    let outside = root.path().join("outside");
    fs::create_dir(&output).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, output.join("Artist")).unwrap();
    let task = s
        .create(
            &json!(123),
            Destination::Named {
                root: output,
                path_type: 1,
                file_type: 0,
            },
            options(),
        )
        .unwrap();
    assert!(s.run(&task.id, Run::default()).await.is_err());
    assert!(b.audio.calls().is_empty());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
