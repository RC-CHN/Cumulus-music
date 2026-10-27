#[path = "support/mv_playback.rs"]
mod fixture;
mod support;
use cumulus_core::{
    Error,
    download::service::{DownloadDestination, DownloadRunOptions, DownloadService},
    mv::{
        MvService,
        playback::{VideoCommand as Cmd, VideoStatus as Status},
    },
    playback::engine::{EndReason, EngineEvent},
};
use fixture::*;
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};
fn open(id: &str) -> Cmd {
    Cmd::Open {
        id: id.into(),
        resolution: 480,
        download_task_id: None,
    }
}
#[tokio::test]
async fn reported_seek_and_checkpoint_use_measured_native_clock_instead_of_requested_target() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.player.command(&token, open("123")).await.unwrap();
    until(&f.player, Status::Playing).await;
    f.last().lock().unwrap().seek_landing = Some(19.75);
    let view = f
        .player
        .command(&token, Cmd::Seek { seconds: 20.0 })
        .await
        .unwrap();
    assert_eq!(view.position, Some(19.75));
    assert_eq!(
        MvService::new(f.session.clone())
            .unwrap()
            .position_for(&token, &json!(123))
            .unwrap()
            .unwrap()
            .position_ms,
        19750
    );
    f.player.command(&token, Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn stop_revokes_playback_and_frames_even_when_checkpoint_storage_fails() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.player.command(&token, open("123")).await.unwrap();
    let view = until(&f.player, Status::Playing).await;
    let device = f.last();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_mv_position BEFORE INSERT ON state WHEN NEW.kind='mv-positions-v1' BEGIN SELECT RAISE(ABORT, 'fixture checkpoint failure'); END;").unwrap();
    assert!(f.player.command(&token, Cmd::Stop).await.is_err());
    assert_eq!(f.player.snapshot().unwrap().status, Status::Idle);
    assert!(device.lock().unwrap().stopped);
    assert!(
        f.player
            .frame(&token, &view.play_id, 0, 320, 180)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn actual_resolution_frames_controls_and_preferences_reopen_from_real_checkpoints() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    assert_eq!(
        f.player.command(&token, open("123")).await.unwrap().status,
        Status::Resolving
    );
    let view = until(&f.player, Status::Playing).await;
    assert_eq!(view.actual_resolution, Some(240));
    assert_eq!(view.requested_resolution, 480);
    let frame = f
        .player
        .frame(&token, &view.play_id, 0, 320, 180)
        .unwrap()
        .unwrap();
    assert_eq!(frame.rgba.len(), 320 * 180 * 4);
    assert!(
        f.player
            .frame(&token, &view.play_id, 1, 320, 180)
            .unwrap()
            .is_none()
    );
    assert!(
        f.player
            .frame(&token, &view.play_id, 0, 1921, 1080)
            .is_err()
    );
    f.player
        .command(&token, Cmd::Seek { seconds: 20.5 })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Pause { paused: true })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Volume { value: 12.0 })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Speed { value: 1.5 })
        .await
        .unwrap();
    f.player.command(&token, Cmd::Stop).await.unwrap();
    assert!(
        f.player
            .frame(&token, &view.play_id, 0, 320, 180)
            .unwrap()
            .is_none()
    );
    let next = f.another();
    assert_eq!(next.snapshot().unwrap().volume, 12.0);
    assert_eq!(next.snapshot().unwrap().speed, 1.5);
    next.command(&token, open("123")).await.unwrap();
    let state = until(&next, Status::Playing).await;
    assert_eq!(state.position, Some(20.5));
    assert_eq!(f.last().lock().unwrap().position, 20.5);
    assert!(!f.last().lock().unwrap().source_file);
    next.command(&token, Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn quick_replacement_and_focus_handoff_revoke_pending_autoplay() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api.hold.store(true, Ordering::Release);
    f.player.command(&token, open("123")).await.unwrap();
    f.api.started.notified().await;
    f.player.command(&token, open("124")).await.unwrap();
    let view = until(&f.player, Status::Playing).await;
    assert_eq!(view.id.as_deref(), Some("124"));
    // The cancelled request may have dropped its waiter. Do not leave a permit
    // which would accidentally release the next, independent held request.
    f.api.release.notify_waiters();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(f.devices.lock().unwrap().len(), 1);
    f.player.command(&token, Cmd::Suspend).await.unwrap();
    assert!(f.last().lock().unwrap().paused);
    f.api.hold.store(true, Ordering::Release);
    f.player.command(&token, open("125")).await.unwrap();
    f.api.started.notified().await;
    f.player.command(&token, Cmd::Suspend).await.unwrap();
    f.api.release.notify_waiters();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(f.player.snapshot().unwrap().status, Status::Idle);
    assert_eq!(f.devices.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn eof_uses_native_entry_and_replay_starts_at_zero() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.player.command(&token, open("123")).await.unwrap();
    until(&f.player, Status::Playing).await;
    f.last().lock().unwrap().events.push(EngineEvent::Ended {
        entry_id: 999,
        reason: EndReason::Eof,
        error: 0,
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(f.player.snapshot().unwrap().status, Status::Playing);
    f.last().lock().unwrap().events.push(EngineEvent::Ended {
        entry_id: 1,
        reason: EndReason::Eof,
        error: 0,
    });
    until(&f.player, Status::Ended).await;
    let checkpoint = MvService::new(f.session.clone())
        .unwrap()
        .position_for(&token, &json!(123))
        .unwrap()
        .unwrap();
    assert!(checkpoint.completed);
    assert_eq!(checkpoint.position_ms, 0);
    f.player
        .command(&token, Cmd::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(f.player.snapshot().unwrap().status, Status::Ended);
    f.player.command(&token, Cmd::Play).await.unwrap();
    let view = until(&f.player, Status::Playing).await;
    assert_eq!(view.position, Some(0.0));
    f.player.command(&token, Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn account_switch_revokes_frames_and_stops_native_engine_without_cross_account_position() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.player.command(&token, open("123")).await.unwrap();
    let view = until(&f.player, Status::Playing).await;
    f.player
        .command(&token, Cmd::Seek { seconds: 21.0 })
        .await
        .unwrap();
    let device = f.last();
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &token)
        .unwrap();
    assert!(matches!(
        f.player.frame(&token, &view.play_id, 0, 320, 180),
        Err(Error::StaleResponse)
    ));
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !device.lock().unwrap().stopped {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let token = f.session.token().unwrap();
    let next = f.another();
    assert_eq!(next.snapshot().unwrap().status, Status::Idle);
    assert!(
        MvService::new(f.session.clone())
            .unwrap()
            .position_for(&token, &json!(123))
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn failures_are_retryable_without_poisoning_valid_control_or_publishing_old_frames() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api.fail_detail.store(true, Ordering::Release);
    f.player.command(&token, open("123")).await.unwrap();
    let failed = until(&f.player, Status::Error).await;
    assert!(failed.error.is_some());
    assert!(
        f.player
            .frame(&token, &failed.play_id, 0, 320, 180)
            .unwrap()
            .is_none()
    );
    f.api.fail_detail.store(false, Ordering::Release);
    f.player.command(&token, Cmd::Play).await.unwrap();
    until(&f.player, Status::Playing).await;
    for command in [
        Cmd::Seek { seconds: f64::NAN },
        Cmd::Seek { seconds: 121.0 },
        Cmd::Volume { value: 101.0 },
        Cmd::Speed { value: 0.0 },
        Cmd::Quality { resolution: 0 },
    ] {
        assert!(f.player.command(&token, command).await.is_err());
        assert_eq!(f.player.snapshot().unwrap().status, Status::Playing);
    }
    f.player
        .command(&token, Cmd::Pause { paused: true })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Seek { seconds: 22.5 })
        .await
        .unwrap();
    f.player
        .command(&token, Cmd::Quality { resolution: 720 })
        .await
        .unwrap();
    let view = until(&f.player, Status::Paused).await;
    assert_eq!(view.requested_resolution, 720);
    assert_eq!(view.actual_resolution, Some(240));
    assert_eq!(view.position, Some(22.5));
    f.player.command(&token, Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn verified_local_mv_survives_network_failure_and_corrupt_explicit_download_is_rejected() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    MvService::new(f.session.clone())
        .unwrap()
        .detail_for(&token, &json!(123))
        .await
        .unwrap();
    let media = Arc::new(support::download::Backend::new(
        include_bytes!("fixtures/mv-seed.mp4").to_vec(),
    ));
    let downloads = DownloadService::with_backend(f.session.clone(), media, 1024 * 1024).unwrap();
    let task = downloads
        .create_mv(
            &json!(123),
            240,
            DownloadDestination::Path {
                path: f.root.path().join("local.mp4"),
            },
        )
        .await
        .unwrap();
    let task = downloads
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    f.api.fail_detail.store(true, Ordering::Release);
    f.player
        .command(
            &token,
            Cmd::Open {
                id: "123".into(),
                resolution: 1080,
                download_task_id: Some(task.id.clone()),
            },
        )
        .await
        .unwrap();
    let view = until(&f.player, Status::Playing).await;
    assert!(view.local);
    assert_eq!(view.actual_resolution, Some(240));
    assert!(f.last().lock().unwrap().source_file);
    f.player.command(&token, Cmd::Stop).await.unwrap();
    std::fs::write(task.target.unwrap(), b"changed external video").unwrap();
    f.player
        .command(
            &token,
            Cmd::Open {
                id: "123".into(),
                resolution: 240,
                download_task_id: Some(task.id),
            },
        )
        .await
        .unwrap();
    until(&f.player, Status::Error).await;
    assert_eq!(f.devices.lock().unwrap().len(), 1);
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/song/enhance/play/mv/url")
    );
}
