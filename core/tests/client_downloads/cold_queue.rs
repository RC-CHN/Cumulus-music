use super::*;
use cumulus_core::{
    download::{
        completed::{FileEvent, FileOptions},
        directory::{DirectoryOptions, DownloadDirectory},
        service::{DownloadAction, DownloadRunOptions, DownloadService},
    },
    storage::OperationLock,
};

fn configure_direct(f: &Fixture) -> DownloadDirectory {
    let directory = DownloadDirectory::new(f.session.clone()).unwrap();
    directory
        .configure(&f.root.path().join("media"), 2, 1)
        .unwrap();
    directory
}

fn service(f: &Fixture) -> DownloadService {
    DownloadService::with_backend(f.session.clone(), f.backend.clone(), 1024 * 1024).unwrap()
}

fn reopen(f: &Fixture) -> MusicClient {
    let registry = ProfileRegistry::open(&f.root.path().join("profile"), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(f.api.clone(), Some(registry.store())));
    session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &session.token().unwrap(),
        )
        .unwrap();
    MusicClient::with_session(session, registry).with_download_backend(f.backend.clone())
}

fn active() -> DownloadQuery {
    DownloadQuery {
        tab: DownloadTab::Active,
        ..Default::default()
    }
}

#[tokio::test]
async fn cold_queue_keeps_interrupted_bytes_until_explicit_resume() {
    let f = Fixture::new();
    configure_direct(&f);
    let s = service(&f);
    let interrupted = s.create_default(&json!(123), options()).unwrap();
    let queued = s.create_default(&json!(124), options()).unwrap();
    let gate = Arc::new(support::transfer::ReadGate::default());
    {
        let mut settings = f.backend.audio.settings.lock().unwrap();
        settings.gate = Some(gate.clone());
        settings.gate_after = 123;
        settings.chunk_limit = 123;
    }
    let worker = {
        let s = s.clone();
        let id = interrupted.id.clone();
        tokio::spawn(async move { s.run(&id, DownloadRunOptions::default()).await })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.reached.notified())
        .await
        .unwrap();
    worker.abort();
    assert!(matches!(worker.await, Err(error) if error.is_cancelled()));
    assert_eq!(
        s.get(&interrupted.id).unwrap().status,
        DownloadStatus::Downloading
    );
    f.backend.audio.settings.lock().unwrap().gate = None;
    let reopened = reopen(&f);
    for _ in 0..2 {
        let view = reopened.downloads("1", active()).await.unwrap();
        assert_eq!(view.active_count, 2);
        assert!(
            view.tasks
                .iter()
                .all(|row| row.status == DownloadStatus::Paused)
        );
        assert_eq!(
            view.tasks
                .iter()
                .find(|row| row.id == interrupted.id)
                .unwrap()
                .bytes,
            123
        );
    }
    assert_eq!(f.api.count("download/url"), 1);
    assert_eq!(f.backend.audio.calls().len(), 1);
    let result = reopened
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![interrupted.id.clone()],
                action: TaskControl::Resume,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.accepted, [interrupted.id]);
    let done = wait(&reopened, "1", DownloadTab::Completed, |v| {
        v.tasks.first().is_some_and(|t| !t.cleanup_pending)
    })
    .await;
    assert_eq!(
        fs::read(done.tasks[0].path.as_ref().unwrap()).unwrap(),
        FLAC
    );
    assert_eq!(f.api.count("download/url"), 2);
    let calls = f.backend.audio.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].offset, 123);
    assert_eq!(s.get(&queued.id).unwrap().status, DownloadStatus::Paused);
}

#[tokio::test]
async fn cold_queue_view_and_fresh_command_do_not_displace_each_other() {
    let f = Fixture::new();
    configure_direct(&f);
    let s = service(&f);
    let old = s.create_default(&json!(777), options()).unwrap();
    let reopened = reopen(&f);
    let (view, command) = tokio::join!(
        reopened.downloads("1", active()),
        reopened.download_command(
            "1",
            DownloadCommand::Start {
                song_ids: vec!["123".into()],
                options: options(),
            },
        ),
    );
    let row = view
        .unwrap()
        .tasks
        .into_iter()
        .find(|row| row.id == old.id)
        .unwrap();
    assert_eq!(row.status, DownloadStatus::Paused);
    let fresh = command.unwrap().accepted;
    assert_eq!(fresh.len(), 1);
    wait(&reopened, "1", DownloadTab::Completed, |v| {
        v.tasks
            .iter()
            .any(|row| row.id == fresh[0] && !row.cleanup_pending)
    })
    .await;
    assert_eq!(f.api.count("download/url"), 1);
    assert_eq!(f.backend.audio.calls().len(), 1);
    assert_eq!(s.get(&old.id).unwrap().status, DownloadStatus::Paused);
}

#[tokio::test]
async fn cold_queue_adopts_linked_output_and_settles_durable_cancel_without_requests() {
    let f = Fixture::new();
    configure_direct(&f);
    let s = service(&f);
    let linked = s.create_default(&json!(123), options()).unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER stop_at_link BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        s.run(&linked.id, DownloadRunOptions::default())
            .await
            .is_err()
    );
    db.execute_batch("DROP TRIGGER stop_at_link").unwrap();
    let target = s.get(&linked.id).unwrap().target.unwrap();
    assert_eq!(fs::read(&target).unwrap(), FLAC);
    let cancelling = s.create_default(&json!(124), options()).unwrap();
    let lock = OperationLock::acquire(
        f.store.directory(),
        "download-task",
        &serde_json::to_string(&("1", &cancelling.id)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        s.request(&cancelling.id, DownloadAction::Cancel)
            .unwrap()
            .status,
        DownloadStatus::Queued
    );
    drop(lock);
    let network_before = f.api.calls.lock().unwrap().len();
    let reopened = reopen(&f);
    let view = reopened
        .downloads(
            "1",
            DownloadQuery {
                tab: DownloadTab::Completed,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(view.completed_count, 1);
    assert_eq!(view.tasks.len(), 1);
    assert_eq!(view.tasks[0].id, linked.id);
    assert!(view.tasks[0].cleanup_pending);
    assert_eq!(view.tasks[0].lyrics_status, LyricsStatus::Pending);
    wait(&reopened, "1", DownloadTab::Active, |v| v.active_count == 0).await;
    assert_eq!(f.api.calls.lock().unwrap().len(), network_before);
    assert_eq!(f.backend.audio.calls().len(), 1);
    assert_eq!(fs::read(&target).unwrap(), FLAC);
    assert_eq!(
        s.get(&cancelling.id).unwrap().status,
        DownloadStatus::Cancelled
    );
    assert!(!cancelling.work.exists());
}

#[tokio::test]
async fn cold_queue_does_not_resume_downloads_when_directory_migration_finishes() {
    let f = Fixture::new();
    let dir = configure_direct(&f);
    let s = service(&f);
    let waiting = s.create_default(&json!(123), options()).unwrap();
    let migration = dir.start(&f.root.path().join("new-media"), None).unwrap();
    let reopened = reopen(&f);
    let view = reopened.downloads("1", active()).await.unwrap();
    assert_eq!(view.tasks[0].status, DownloadStatus::Paused);
    assert_eq!(view.migration.unwrap().id, migration.id);
    reopened
        .download_command(
            "1",
            DownloadCommand::Migration {
                operation_id: migration.id,
                cancel: false,
            },
        )
        .await
        .unwrap();
    let result = wait(&reopened, "1", DownloadTab::Active, |v| {
        v.migration.is_none()
    })
    .await;
    assert_eq!(result.tasks[0].status, DownloadStatus::Paused);
    assert_eq!(f.api.count("download/url"), 0);
    assert!(f.backend.audio.calls().is_empty());
    let moved = s.get(&waiting.id).unwrap();
    assert!(
        moved
            .work
            .starts_with(f.root.path().join("new-media").canonicalize().unwrap())
    );
    assert!(moved.work.is_dir());
}

#[tokio::test]
async fn cold_queue_second_runtime_cannot_freeze_a_live_download() {
    let f = Fixture::new();
    let gate = Arc::new(support::transfer::ReadGate::default());
    f.backend.audio.settings.lock().unwrap().gate = Some(gate.clone());
    let id = f.start().await;
    tokio::time::timeout(Duration::from_secs(3), gate.reached.notified())
        .await
        .unwrap();
    let second = reopen(&f);
    assert!(matches!(
        second.downloads("1", active()).await,
        Err(Error::MutationBusy)
    ));
    let same_host = f.client.downloads("1", active()).await.unwrap();
    assert_eq!(same_host.tasks[0].id, id);
    assert_eq!(same_host.tasks[0].status, DownloadStatus::Downloading);
    assert_eq!(f.api.count("download/url"), 1);
    gate.release.notify_one();
    let finished = wait(&f.client, "1", DownloadTab::Completed, |view| {
        view.tasks.first().is_some_and(|task| !task.cleanup_pending)
    })
    .await;
    assert_eq!(finished.tasks[0].id, id);
    assert_eq!(f.backend.audio.calls().len(), 1);
}

#[tokio::test]
async fn cold_queue_recovers_a_failed_raw_checksum_without_reauthorization() {
    let f = Fixture::new();
    configure_direct(&f);
    let s = service(&f);
    let task = s.create_default(&json!(123), options()).unwrap();
    f.api.resource.lock().unwrap()["md5"] = json!("00000000000000000000000000000000");
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER stop_at_integrity_error BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='error' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        s.run(&task.id, DownloadRunOptions::default())
            .await
            .is_err()
    );
    db.execute_batch("DROP TRIGGER stop_at_integrity_error")
        .unwrap();
    assert_eq!(s.get(&task.id).unwrap().status, DownloadStatus::Downloading);
    let reopened = reopen(&f);
    let view = reopened.downloads("1", active()).await.unwrap();
    assert_eq!(view.tasks[0].id, task.id);
    assert_eq!(view.tasks[0].status, DownloadStatus::Error);
    assert!(view.tasks[0].needs_transfer_restart);
    assert_eq!(f.api.count("download/url"), 1);
    assert_eq!(f.backend.audio.calls().len(), 1);
}

#[tokio::test]
async fn cold_queue_preserves_a_staged_move_before_settling_an_existing_cancel() {
    let f = Fixture::new();
    let dir = configure_direct(&f).with_options(DirectoryOptions {
        files: FileOptions {
            copy_only: true,
            observer: Some(Arc::new(|event, _| {
                if event == FileEvent::DestinationReady {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            })),
        },
        ..Default::default()
    });
    let s = service(&f);
    let task = s.create_default(&json!(123), options()).unwrap();
    let paused = s
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(31),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    let migration = dir.start(&f.root.path().join("new-media"), None).unwrap();
    assert!(matches!(dir.resume(&migration.id), Err(Error::Cancelled)));
    let before = s.request(&task.id, DownloadAction::Cancel).unwrap();
    assert!(before.file_operation.is_some());
    assert_eq!(before.status, DownloadStatus::Paused);
    let row = f.store.get("1", "download_tasks_v1", &task.id).unwrap();
    let reopened = reopen(&f);
    let view = reopened.downloads("1", active()).await.unwrap();
    assert_eq!(view.tasks[0].status, DownloadStatus::Paused);
    assert_eq!(view.tasks[0].file_operation, before.file_operation);
    assert_eq!(
        f.store.get("1", "download_tasks_v1", &task.id).unwrap(),
        row
    );
    reopened
        .download_command(
            "1",
            DownloadCommand::Migration {
                operation_id: migration.id,
                cancel: false,
            },
        )
        .await
        .unwrap();
    wait(&reopened, "1", DownloadTab::Active, |v| {
        v.migration.is_none() && v.active_count == 0
    })
    .await;
    assert_eq!(s.get(&task.id).unwrap().status, DownloadStatus::Cancelled);
    assert_eq!(f.api.count("download/url"), 1);
    assert_eq!(f.backend.audio.calls().len(), 1);
}
