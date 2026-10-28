use super::*;
use cumulus_core::client::native_media::{
    NativeMediaAdjustment as Adjust, NativeMediaKind, NativeMediaStatus,
};

#[tokio::test]
async fn native_details_are_lazy_sanitized_and_stable_across_clock_and_controls() {
    let f = Fixture::new();
    let idle = f.client.native_media_details().unwrap();
    assert!(idle.item.is_none() && !idle.can_seek);
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(f.video.devices.lock().unwrap().is_empty());
    assert!(f.video.api.calls.lock().unwrap().is_empty());
    f.client.play_selection("1", songs()).await.unwrap();
    let first = f.client.native_media_details().unwrap();
    assert_eq!(first.state, NativeMediaStatus::Playing);
    let item = first.item.as_ref().unwrap();
    assert_eq!(item.kind, NativeMediaKind::Music);
    assert_eq!(item.title, "Song");
    assert_eq!(item.artists, ["Artist"]);
    assert_eq!(item.album, "Album");
    assert_eq!(
        item.artwork.as_deref(),
        Some("https://p1.music.126.net/cover.jpg?param=320y320")
    );
    assert_eq!(item.id.len(), 64);
    assert!(!format!("{item:?}").contains("SECRET"));
    assert!(first.can_seek);
    assert_eq!(first.duration, Some(120.0));
    assert_eq!(first.seek_start, 0.0);
    let target = first.transport.target.unwrap();
    f.client
        .native_media_adjust(&target, Adjust::Seek { seconds: 17.5 })
        .await
        .unwrap();
    let after = f.client.native_media_details().unwrap();
    assert_eq!(after.item.as_ref().unwrap().id, item.id);
    assert_eq!(after.position, Some(17.5));
    assert_ne!(after.seek_revision, first.seek_revision);
    assert!(matches!(
        f.client
            .native_media_adjust(&target, Adjust::Volume { value: 10.0 })
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.audio.state.lock().unwrap().view.volume, Some(100.0));
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = f.client.native_media_details().unwrap();
    assert_eq!(paused.state, NativeMediaStatus::Paused);
    assert_eq!(paused.seek_revision, after.seek_revision);
    assert_eq!(paused.item.as_ref().unwrap().id, item.id);
    f.client.play_selection("1", songs()).await.unwrap();
    assert_ne!(
        f.client.native_media_details().unwrap().item.unwrap().id,
        item.id
    );
}

#[tokio::test]
async fn native_adjustments_preserve_pause_and_persist_through_ordinary_use_cases() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    for adjustment in [
        Adjust::Volume { value: 37.0 },
        Adjust::Speed { value: 1.5 },
        Adjust::Mode {
            mode: cumulus_core::playback::PlaybackMode::SingleLoop,
        },
    ] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        f.client
            .native_media_adjust(&target, adjustment)
            .await
            .unwrap();
    }
    let current = f.client.native_media_details().unwrap();
    assert_eq!(current.seek_revision, 0);
    assert_eq!(current.volume, 37.0);
    assert_eq!(current.speed, 1.5);
    assert_eq!(
        current.mode,
        Some(cumulus_core::playback::PlaybackMode::SingleLoop)
    );
    assert_eq!(
        f.client.native_media_details().unwrap().state,
        NativeMediaStatus::Paused
    );
    let saved = cumulus_core::storage::StateStore::open(f._dir.path())
        .unwrap()
        .get("9", "playback", "preferences")
        .unwrap()
        .unwrap();
    assert_eq!(saved["volume"], 37.0);
    assert_eq!(saved["speed"], 1.5);
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    for adjustment in [
        Adjust::Seek { seconds: f64::NAN },
        Adjust::Seek { seconds: 121.0 },
        Adjust::Volume { value: -1.0 },
        Adjust::Volume {
            value: f64::INFINITY,
        },
        Adjust::Speed { value: 0.0 },
        Adjust::Speed { value: 4.01 },
    ] {
        assert!(matches!(
            f.client.native_media_adjust(&target, adjustment).await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            f.client.native_media_snapshot().unwrap().target.as_ref(),
            Some(&target)
        );
    }
}

#[tokio::test]
async fn native_preferences_report_disk_failure_and_restore_the_live_engine() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let db = rusqlite::Connection::open(f._dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_media_preferences BEFORE INSERT ON state WHEN NEW.kind='playback' AND NEW.key='preferences' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
    for adjustment in [Adjust::Volume { value: 12.0 }, Adjust::Speed { value: 2.0 }] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        assert!(
            f.client
                .native_media_adjust(&target, adjustment)
                .await
                .is_err()
        );
        let view = f.client.native_media_details().unwrap();
        assert_eq!((view.volume, view.speed), (100.0, 1.0));
        assert_eq!(view.state, NativeMediaStatus::Playing);
        let device = f.audio.state.lock().unwrap();
        assert_eq!(device.view.volume, Some(100.0));
        assert_eq!(device.view.speed, Some(1.0));
    }
    db.execute_batch("DROP TRIGGER reject_media_preferences;")
        .unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .native_media_adjust(&target, Adjust::Volume { value: 12.0 })
        .await
        .unwrap();
    assert_eq!(f.client.native_media_details().unwrap().volume, 12.0);
}

#[tokio::test]
async fn native_video_preferences_do_not_survive_a_rejected_persistent_write() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    let before = f.client.native_media_details().unwrap();
    let db = rusqlite::Connection::open(f._dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_video_preferences BEFORE INSERT ON state WHEN NEW.kind='mv-player-v1' AND NEW.key='settings' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
    for adjustment in [Adjust::Volume { value: 12.0 }, Adjust::Speed { value: 2.0 }] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        assert!(
            f.client
                .native_media_adjust(&target, adjustment)
                .await
                .is_err()
        );
        let view = f.client.native_media_details().unwrap();
        assert_eq!((view.volume, view.speed), (before.volume, before.speed));
        let device = f.video.last();
        let device = device.lock().unwrap();
        assert_eq!((device.volume, device.speed), (before.volume, before.speed));
    }
    db.execute_batch("DROP TRIGGER reject_video_preferences;")
        .unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .native_media_adjust(&target, Adjust::Volume { value: 12.0 })
        .await
        .unwrap();
    assert_eq!(f.client.native_media_details().unwrap().volume, 12.0);
}

#[tokio::test]
async fn failed_video_preference_rollback_retires_output_until_explicit_recovery() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    let before = f.client.native_media_details().unwrap();
    let device = f.video.last();
    device.lock().unwrap().reject_volume = Some(before.volume);
    let db = rusqlite::Connection::open(f._dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_video_preferences BEFORE INSERT ON state WHEN NEW.kind='mv-player-v1' AND NEW.key='settings' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    assert!(
        f.client
            .native_media_adjust(&target, Adjust::Volume { value: 12.0 })
            .await
            .is_err()
    );
    let view = f.client.video_snapshot("1").await.unwrap();
    assert_eq!(view.state, VideoStatus::Error);
    assert!(device.lock().unwrap().stopped);
    assert!(!f.client.native_media_details().unwrap().can_seek);
    db.execute_batch("DROP TRIGGER reject_video_preferences;")
        .unwrap();
    f.client.video_action("1", VideoAction::Play).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert_eq!(f.video.last().lock().unwrap().volume, before.volume);
    assert_eq!(
        f.client.native_media_details().unwrap().volume,
        before.volume
    );
}

#[tokio::test]
async fn native_preview_projection_and_seek_keep_authorized_full_song_coordinates() {
    let f = Fixture::new();
    f.api.trial.store(true, Ordering::Release);
    f.client.play_selection("1", songs()).await.unwrap();
    let view = f.client.native_media_details().unwrap();
    assert_eq!(view.seek_start, 30.0);
    assert_eq!(view.seek_end, Some(60.0));
    assert_eq!(view.duration, Some(120.0));
    let target = view.transport.target.unwrap();
    for seconds in [0.0, 29.9, 60.1] {
        assert!(matches!(
            f.client
                .native_media_adjust(&target, Adjust::Seek { seconds })
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    f.client
        .native_media_adjust(&target, Adjust::Seek { seconds: 40.0 })
        .await
        .unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().position,
        Some(40.0)
    );
    assert_eq!(f.audio.state.lock().unwrap().view.position, Some(10.0));
}

#[tokio::test]
async fn native_video_adjustments_use_video_only_and_reject_account_handoffs() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let audio = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client
            .native_media_adjust(&audio, Adjust::Volume { value: 5.0 })
            .await,
        Err(Error::StaleResponse)
    ));
    let details = f.client.native_media_details().unwrap();
    assert_eq!(details.item.as_ref().unwrap().kind, NativeMediaKind::Video);
    assert!(details.can_seek && details.mode.is_none());
    assert!(!details.transport.can_next);
    let id = details.item.unwrap().id;
    for adjustment in [
        Adjust::Seek { seconds: 18.0 },
        Adjust::Volume { value: 42.0 },
        Adjust::Speed { value: 2.0 },
    ] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        f.client
            .native_media_adjust(&target, adjustment)
            .await
            .unwrap();
    }
    assert_eq!(f.video.last().lock().unwrap().volume, 42.0);
    assert_eq!(f.video.last().lock().unwrap().speed, 2.0);
    assert_eq!(f.audio.state.lock().unwrap().view.volume, Some(100.0));
    assert!(f.audio.state.lock().unwrap().view.paused);
    let details = f.client.native_media_details().unwrap();
    assert_eq!(details.item.unwrap().id, id);
    let target = details.transport.target.unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client
            .native_media_adjust(&target, Adjust::Seek { seconds: 2.0 })
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.client.native_media_details().unwrap().item.is_none());
}

#[tokio::test]
async fn system_video_play_preserves_pause_position_and_resets_after_stop() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    f.client
        .video_action("1", VideoAction::Seek { seconds: 18.0 })
        .await
        .unwrap();
    f.client
        .video_action("1", VideoAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.native_media_play(&paused).await.unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().position,
        Some(18.0)
    );
    f.client.video_action("1", VideoAction::Stop).await.unwrap();
    let stopped = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.native_media_play(&stopped).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert_eq!(f.client.native_media_details().unwrap().position, Some(0.0));
    assert!(matches!(
        f.client.native_media_play(&paused).await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn system_play_restarts_trial_at_the_authorized_start_and_preserves_paused_seek() {
    let f = Fixture::new();
    f.api.trial.store(true, Ordering::Release);
    f.client.play_selection("1", songs()).await.unwrap();
    f.client
        .player_action("1", PlayerAction::Seek { seconds: 45.0 })
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.native_media_play(&target).await.unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().position,
        Some(45.0)
    );
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.native_media_play(&target).await.unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().position,
        Some(30.0)
    );
    assert_eq!(f.audio.state.lock().unwrap().view.position, Some(0.0));
}
