#[path = "support/mv_playback.rs"]
mod fixture;
mod support;
use cumulus_core::{
    Error,
    download::{
        completed::{CompletedDownloads, FileAction, FileEvent, FileOptions, FilePhase},
        directory::{DirectoryEvent, DirectoryOptions, DownloadDirectory, MigrationPhase},
        service::{DownloadDestination, DownloadRunOptions, DownloadService, DownloadTask},
    },
    mv::playback::{VideoCommand as Cmd, VideoStatus as Status},
    storage::StateStore,
};
use fixture::*;
use serde_json::json;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

async fn local(f: &Fixture) -> (DownloadService, DownloadTask, String) {
    let (downloads, task) = download(f).await;
    f.player
        .command(
            &f.session.token().unwrap(),
            Cmd::Open {
                id: "123".into(),
                resolution: 480,
                download_task_id: Some(task.id.clone()),
            },
        )
        .await
        .unwrap();
    let play = until(&f.player, Status::Playing).await;
    assert!(play.local);
    f.player
        .command(&f.session.token().unwrap(), Cmd::Seek { seconds: 20.5 })
        .await
        .unwrap();
    (downloads, task, play.play_id)
}
async fn download(f: &Fixture) -> (DownloadService, DownloadTask) {
    let media = Arc::new(support::download::Backend::new(
        include_bytes!("fixtures/mv-seed.mp4").to_vec(),
    ));
    let downloads = DownloadService::with_backend(f.session.clone(), media, 1024 * 1024).unwrap();
    let task = downloads
        .create_mv(
            &json!(123),
            240,
            DownloadDestination::Path {
                path: f.root.path().join("media/local.mp4"),
            },
        )
        .await
        .unwrap();
    let task = downloads
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    (downloads, task)
}

#[tokio::test]
async fn file_intent_cancels_a_pending_decoder_and_focus_handoff_keeps_recovery_paused() {
    let f = Fixture::new();
    let (downloads, task) = download(&f).await;
    f.hold_load.store(true, Ordering::Release);
    let token = f.session.token().unwrap();
    f.player
        .command(
            &token,
            Cmd::Open {
                id: "123".into(),
                resolution: 240,
                download_task_id: Some(task.id.clone()),
            },
        )
        .await
        .unwrap();
    f.load_started.notified().await;
    let op = files(&f).rename(&task.id, "during-load.mp4").unwrap();
    until(&f.player, Status::FileOperation).await;
    f.player.command(&token, Cmd::Suspend).await.unwrap();
    f.hold_load.store(false, Ordering::Release);
    let worker = files(&f);
    tokio::task::spawn_blocking(move || worker.resume(&op.id))
        .await
        .unwrap()
        .unwrap();
    until(&f.player, Status::Paused).await;
    assert_eq!(
        f.last().lock().unwrap().source_path,
        downloads.get(&task.id).unwrap().target
    );
    assert!(f.devices.lock().unwrap()[0].lock().unwrap().stopped);
    no_remote_play(&f);
    f.player.shutdown().await.unwrap();
}

#[tokio::test]
async fn checkpoint_failure_still_releases_file_and_restart_recovers_pending_journal() {
    let f = Fixture::new();
    let (downloads, task, _) = local(&f).await;
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_mv_position BEFORE INSERT ON state WHEN NEW.kind='mv-positions-v1' BEGIN SELECT RAISE(ABORT, 'fixture checkpoint failure'); END;").unwrap();
    let op = files(&f).rename(&task.id, "after-restart.mp4").unwrap();
    until(&f.player, Status::Error).await;
    assert!(f.last().lock().unwrap().stopped);
    f.player.shutdown().await.unwrap();
    db.execute_batch("DROP TRIGGER reject_mv_position").unwrap();
    // Reopen all persisted state with a fresh session and actor, as at startup.
    let session = Arc::new(cumulus_core::account::AccountSession::new(
        f.api.clone(),
        Some(Arc::new(StateStore::open(f.store.directory()).unwrap())),
    ));
    session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &session.token().unwrap(),
        )
        .unwrap();
    let player = cumulus_core::mv::playback::VideoController::start(
        session.clone(),
        session.token().unwrap(),
        f.factory.clone(),
    )
    .unwrap();
    let open = || Cmd::Open {
        id: "123".into(),
        resolution: 240,
        download_task_id: Some(task.id.clone()),
    };
    player
        .command(&session.token().unwrap(), open())
        .await
        .unwrap();
    until(&player, Status::Error).await;
    assert_eq!(f.devices.lock().unwrap().len(), 1);
    let worker = reopen(&f);
    tokio::task::spawn_blocking(move || worker.resume(&op.id))
        .await
        .unwrap()
        .unwrap();
    player
        .command(&session.token().unwrap(), open())
        .await
        .unwrap();
    let view = until(&player, Status::Playing).await;
    assert_eq!(view.position, Some(20.5));
    assert_eq!(
        f.last().lock().unwrap().source_path,
        downloads.get(&task.id).unwrap().target
    );
    no_remote_play(&f);
    player.shutdown().await.unwrap();
}
fn files(f: &Fixture) -> CompletedDownloads {
    CompletedDownloads::new(f.session.clone()).unwrap()
}
fn reopen(f: &Fixture) -> CompletedDownloads {
    CompletedDownloads::offline(
        Arc::new(StateStore::open(f.store.directory()).unwrap()),
        "1",
    )
    .unwrap()
}
fn no_remote_play(f: &Fixture) {
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/song/enhance/play/mv/url")
    );
}

#[tokio::test]
async fn rename_waits_for_decoder_release_and_restores_position_and_preferences() {
    let f = Fixture::new();
    let (downloads, task, old_play) = local(&f).await;
    let token = f.session.token().unwrap();
    f.player
        .command(&token, Cmd::Volume { value: 27.0 })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Speed { value: 1.25 })
        .await
        .unwrap();
    let old = f.last();
    let (hold, started) = {
        let d = old.lock().unwrap();
        d.hold_stop.store(true, Ordering::Release);
        (d.hold_stop.clone(), d.stop_started.clone())
    };
    let op = files(&f).rename(&task.id, "renamed.mp4").unwrap();
    let worker = files(&f);
    let run = tokio::task::spawn_blocking(move || worker.resume(&op.id));
    tokio::time::timeout(Duration::from_secs(4), started.notified())
        .await
        .unwrap();
    assert!(!run.is_finished());
    assert!(task.target.as_ref().unwrap().exists());
    assert_eq!(downloads.get(&task.id).unwrap().target, task.target);
    assert!(
        f.player
            .frame(&token, &old_play, 0, 128, 72)
            .unwrap()
            .is_none()
    );
    hold.store(false, Ordering::Release);
    assert_eq!(run.await.unwrap().unwrap().phase, FilePhase::Complete);
    let view = until(&f.player, Status::Playing).await;
    assert_ne!(view.play_id, old_play);
    assert_eq!(view.position, Some(20.5));
    assert_eq!((view.volume, view.speed), (27.0, 1.25));
    assert_eq!(
        f.last().lock().unwrap().source_path,
        downloads.get(&task.id).unwrap().target
    );
    assert!(old.lock().unwrap().stopped);
    assert!(!task.target.unwrap().exists());
    no_remote_play(&f);
    f.player.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupted_move_reopens_journal_and_restores_paused_local_source() {
    let f = Fixture::new();
    let (downloads, task, _) = local(&f).await;
    let token = f.session.token().unwrap();
    f.player
        .command(&token, Cmd::Pause { paused: true })
        .await
        .unwrap();
    let target = f.root.path().join("moved/final.mp4");
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    let op = files(&f)
        .start(&task.id, FileAction::Move, Some(target.clone()))
        .unwrap();
    let id = op.id.clone();
    let worker = files(&f).with_options(FileOptions {
        copy_only: true,
        observer: Some(Arc::new(|event, _| {
            if event == FileEvent::MetadataCommitted {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        })),
    });
    assert!(
        tokio::task::spawn_blocking(move || worker.resume(&id))
            .await
            .unwrap()
            .is_err()
    );
    until(&f.player, Status::FileOperation).await;
    assert_eq!(files(&f).get(&op.id).unwrap().phase, FilePhase::Cleanup);
    assert!(target.exists() && task.target.as_ref().unwrap().exists());
    assert_eq!(
        downloads.get(&task.id).unwrap().target,
        Some(target.clone())
    );
    let worker = reopen(&f);
    tokio::task::spawn_blocking(move || worker.resume(&op.id))
        .await
        .unwrap()
        .unwrap();
    let view = until(&f.player, Status::Paused).await;
    assert_eq!(view.position, Some(20.5));
    assert_eq!(f.last().lock().unwrap().source_path, Some(target));
    assert!(!task.target.unwrap().exists());
    no_remote_play(&f);
    f.player.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_copy_restores_original_and_honors_pause_during_file_operation() {
    let f = Fixture::new();
    let (_, task, _) = local(&f).await;
    let target = f.root.path().join("media/copy.mp4");
    let op = files(&f)
        .start(&task.id, FileAction::Move, Some(target.clone()))
        .unwrap();
    let worker = files(&f).with_options(FileOptions {
        copy_only: true,
        observer: Some(Arc::new(|event, _| {
            if event == FileEvent::Staged {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        })),
    });
    let id = op.id.clone();
    assert!(
        tokio::task::spawn_blocking(move || worker.resume(&id))
            .await
            .unwrap()
            .is_err()
    );
    until(&f.player, Status::FileOperation).await;
    f.player
        .command(&f.session.token().unwrap(), Cmd::Pause { paused: true })
        .await
        .unwrap();
    let worker = reopen(&f);
    tokio::task::spawn_blocking(move || worker.cancel(&op.id))
        .await
        .unwrap()
        .unwrap();
    let view = until(&f.player, Status::Paused).await;
    assert_eq!(view.position, Some(20.5));
    assert_eq!(f.last().lock().unwrap().source_path, task.target);
    assert!(!target.exists());
    no_remote_play(&f);
    f.player.shutdown().await.unwrap();
}

#[tokio::test]
async fn delete_and_forget_stop_active_video_without_network_fallback() {
    for action in [FileAction::Delete, FileAction::Forget] {
        let f = Fixture::new();
        let (downloads, task, play) = local(&f).await;
        let old = f.last();
        let op = files(&f).start(&task.id, action, None).unwrap();
        let worker = files(&f);
        tokio::task::spawn_blocking(move || worker.resume(&op.id))
            .await
            .unwrap()
            .unwrap();
        until(&f.player, Status::Idle).await;
        assert!(downloads.list().unwrap().is_empty());
        assert_eq!(task.target.unwrap().exists(), action == FileAction::Forget);
        assert!(old.lock().unwrap().stopped);
        assert!(
            f.player
                .frame(&f.session.token().unwrap(), &play, 0, 128, 72)
                .unwrap()
                .is_none()
        );
        no_remote_play(&f);
        f.player.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn directory_commit_and_rollback_wait_for_whole_batch_before_resuming() {
    for rollback in [false, true] {
        let f = Fixture::new();
        let (downloads, task, _) = local(&f).await;
        let directory = DownloadDirectory::new(f.session.clone()).unwrap();
        directory
            .configure(&f.root.path().join("media"), 0, 0)
            .unwrap();
        let op = directory
            .start(&f.root.path().join("migrated"), None)
            .unwrap();
        let id = op.id.clone();
        let worker = directory.clone().with_options(DirectoryOptions {
            files: FileOptions {
                copy_only: true,
                ..Default::default()
            },
            observer: Some(Arc::new(|event, _| {
                if event == DirectoryEvent::ItemComplete {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            })),
            ..Default::default()
        });
        assert!(
            tokio::task::spawn_blocking(move || worker.resume(&id))
                .await
                .unwrap()
                .is_err()
        );
        until(&f.player, Status::FileOperation).await;
        // The child task is already committed and unlocked, but the batch can
        // still roll it back. No decoder may open this intermediate path.
        assert!(downloads.get(&task.id).unwrap().file_operation.is_none());
        assert_eq!(f.devices.lock().unwrap().len(), 1);
        let worker = DownloadDirectory::offline(
            Arc::new(StateStore::open(f.store.directory()).unwrap()),
            "1",
        )
        .unwrap();
        let finished = tokio::task::spawn_blocking(move || {
            if rollback {
                worker.cancel(&op.id)
            } else {
                worker.resume(&op.id)
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            finished.phase,
            if rollback {
                MigrationPhase::Cancelled
            } else {
                MigrationPhase::Complete
            }
        );
        let view = until(&f.player, Status::Playing).await;
        assert_eq!(view.position, Some(20.5));
        let path = downloads.get(&task.id).unwrap().target;
        assert_eq!(f.last().lock().unwrap().source_path, path);
        assert_eq!(path == task.target, rollback);
        no_remote_play(&f);
        f.player.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn stop_replacement_and_account_change_revoke_waiting_local_recovery() {
    for change in ["stop", "replace", "account"] {
        let f = Fixture::new();
        let (_, task, _) = local(&f).await;
        let op = files(&f).rename(&task.id, "later.mp4").unwrap();
        until(&f.player, Status::FileOperation).await;
        let token = f.session.token().unwrap();
        match change {
            "stop" => {
                f.player.command(&token, Cmd::Stop).await.unwrap();
            }
            "replace" => {
                f.player
                    .command(
                        &token,
                        Cmd::Open {
                            id: "124".into(),
                            resolution: 240,
                            download_task_id: None,
                        },
                    )
                    .await
                    .unwrap();
                until(&f.player, Status::Playing).await;
            }
            _ => {
                f.session
                    .adopt(f.api.clone(), json!({"userId":2}), &token)
                    .unwrap();
            }
        }
        let worker = reopen(&f);
        tokio::task::spawn_blocking(move || worker.resume(&op.id))
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        if change == "account" {
            assert!(f.player.snapshot().is_err());
        } else if change == "replace" {
            assert_eq!(f.player.snapshot().unwrap().id.as_deref(), Some("124"));
        } else {
            assert_eq!(f.player.snapshot().unwrap().status, Status::Idle);
        }
        assert_eq!(
            f.devices.lock().unwrap().len(),
            if change == "replace" { 2 } else { 1 }
        );
        f.player.shutdown().await.unwrap();
    }
}
