use super::*;

#[tokio::test]
async fn a_rejected_native_fm_mode_change_does_not_cancel_an_inflight_next_track() {
    use cumulus_core::client::native_media::NativeMediaAdjustment;
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client.player_action("1", PlayerAction::Next).await.unwrap();
    // Recovered manual FM steps have a 250ms debounce. The second step reaches
    // the refill threshold; hold its real synthetic recommendation response.
    tokio::time::sleep(std::time::Duration::from_millis(260)).await;
    api.batch(&[5, 6, 7]);
    *api.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let next_client = client.clone();
    let next =
        tokio::spawn(async move { next_client.player_action("1", PlayerAction::Next).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    let target = client.native_media_snapshot().unwrap().target.unwrap();
    let rejected = client
        .native_media_adjust(
            &target,
            NativeMediaAdjustment::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await;
    api.release.notify_one();
    assert!(matches!(rejected, Err(Error::MutationBusy)));
    let changed = next
        .await
        .unwrap()
        .expect("a rejected mode must not supersede the accepted Next");
    assert!(changed.fm);
    assert_eq!(changed.track.unwrap().id, "3");
    assert_eq!(changed.mode, PlaybackMode::Sequential);
    assert_eq!(api.count("/api/v1/radio/get"), 2);
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}

#[tokio::test]
async fn mini_surface_keeps_fm_queue_owned_by_the_coordinator() {
    use cumulus_core::client::mini::MiniAction;
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3, 4]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let mini = client.mini_snapshot().unwrap();
    assert_eq!(mini.view.mode, Some(PlaybackMode::Sequential));
    assert_eq!(
        mini.view.mode_family,
        Some(cumulus_core::playback::mode::ModeFamily::Fm)
    );
    assert!(!mini.view.can_intelligence);
    let queue = client.mini_queue(&mini, 0, 30).await.unwrap();
    assert!(!queue.editable);
    for action in [
        MiniAction::Clear,
        MiniAction::Select {
            entry_id: queue.tracks[1].entry_id.clone(),
        },
        MiniAction::Remove {
            entry_id: queue.tracks[1].entry_id.clone(),
        },
        MiniAction::Mode {
            mode: PlaybackMode::Random,
        },
    ] {
        assert!(client.mini_action(&mini, action).await.is_err());
    }
    client.mini_action(&mini, MiniAction::Next).await.unwrap();
    let current = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(current.fm);
    assert_eq!(current.track.unwrap().id, "2");
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}

#[tokio::test]
async fn mini_fm_repeat_preserves_paused_audio_and_rejects_old_or_non_fm_choices() {
    use cumulus_core::playback::controller::PlaybackStatus;
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let mut stream = client.subscribe_playback("1").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while stream.current().unwrap().state != PlaybackStatus::Playing {
            stream.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
    let calls = api.calls.lock().unwrap().len();
    for mode in [
        PlaybackMode::SingleLoop,
        PlaybackMode::Sequential,
        PlaybackMode::SingleLoop,
    ] {
        let displayed = client.mini_snapshot().unwrap();
        for invalid in [PlaybackMode::Random, PlaybackMode::OrderLoop] {
            assert!(client.mini_mode(&displayed, invalid).await.is_err());
        }
        let receipt = client.mini_mode(&displayed, mode).await.unwrap();
        assert!(receipt.fm);
        assert_eq!(receipt.mode, mode);
        assert_eq!(receipt.play_id, before.play_id);
        let after = stream.current().unwrap();
        assert_eq!(after.state, PlaybackStatus::Paused);
        assert_eq!(after.position, before.position);
        assert!(!displayed.same_controls(&client.mini_snapshot().unwrap()));
        assert!(matches!(
            client.mini_mode(&displayed, mode).await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn mini_empty_fm_retains_modes_and_a_readonly_empty_queue_with_its_own_fence() {
    let (_dir, api, _session, client) = fixture();
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let empty = client.mini_snapshot().unwrap();
    assert!(empty.view.item.is_none());
    assert_eq!(
        empty.view.mode_family,
        Some(cumulus_core::playback::mode::ModeFamily::Fm)
    );
    let queue = client.mini_queue(&empty, 0, 30).await.unwrap();
    assert!(queue.tracks.is_empty() && !queue.editable);
    let calls = api.calls.lock().unwrap().len();
    let repeated = client
        .mini_mode(&empty, PlaybackMode::SingleLoop)
        .await
        .unwrap();
    assert!(repeated.fm && repeated.play_id.is_none());
    assert_eq!(repeated.mode, PlaybackMode::SingleLoop);
    let current = client.mini_snapshot().unwrap();
    assert!(!empty.same_controls(&current));
    assert!(matches!(
        client.mini_mode(&empty, PlaybackMode::Sequential).await,
        Err(Error::StaleResponse)
    ));
    client
        .mini_mode(&current, PlaybackMode::Sequential)
        .await
        .unwrap();
    let last_fm = client.mini_snapshot().unwrap();
    client
        .fm_command("1", FmSource::Personal, FmCommand::Leave)
        .await
        .unwrap();
    assert!(client.mini_snapshot().unwrap().view.mode.is_none());
    assert!(matches!(
        client.mini_mode(&last_fm, PlaybackMode::SingleLoop).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn mini_empty_fm_source_and_account_changes_reject_the_old_mode_without_http() {
    let (_dir, api, session, client) = fixture();
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let previous = client.mini_snapshot().unwrap();
    client
        .fm_command(
            "1",
            FmSource::Zone {
                rule_id: "27001".into(),
                trial_mode: None,
            },
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let current = client.mini_snapshot().unwrap();
    assert!(!previous.same_controls(&current));
    let calls = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.mini_mode(&previous, PlaybackMode::SingleLoop).await,
        Err(Error::StaleResponse)
    ));
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.mini_mode(&current, PlaybackMode::SingleLoop).await,
        Err(Error::StaleResponse)
    ));
    assert!(client.mini_snapshot().unwrap().view.mode.is_none());
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn mini_fm_failed_actor_commit_retries_the_saved_repeat_and_survives_restart() {
    let (dir, api, session, client) = fixture();
    api.batch(&[1, 2, 3]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let sql = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_mini_fm BEFORE UPDATE ON state WHEN NEW.owner='9' AND NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic FM mode failure'); END;").unwrap();
    let displayed = client.mini_snapshot().unwrap();
    assert!(
        client
            .mini_mode(&displayed, PlaybackMode::SingleLoop)
            .await
            .is_err()
    );
    assert_eq!(
        client.mini_snapshot().unwrap().view.mode,
        Some(PlaybackMode::Sequential)
    );
    assert!(
        client
            .fm_view("1", FmSource::Personal, false)
            .await
            .unwrap()
            .value
            .mode_pending
    );
    assert_eq!(
        client
            .mini_snapshot()
            .unwrap()
            .view
            .fm_mode_pending
            .unwrap()
            .mode,
        PlaybackMode::SingleLoop
    );
    sql.execute_batch("DROP TRIGGER reject_mini_fm;").unwrap();
    let fresh = client.mini_snapshot().unwrap();
    let repaired = client
        .mini_mode(&fresh, PlaybackMode::SingleLoop)
        .await
        .unwrap();
    assert!(repaired.fm);
    assert_eq!(repaired.mode, PlaybackMode::SingleLoop);
    assert!(
        client
            .mini_snapshot()
            .unwrap()
            .view
            .fm_mode_pending
            .is_none()
    );
    assert!(
        !client
            .fm_view("1", FmSource::Personal, false)
            .await
            .unwrap()
            .value
            .mode_pending
    );
    drop(client);
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened = MusicClient::with_session(session, registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    let calls = api.calls.lock().unwrap().len();
    assert!(
        reopened.mini_snapshot().unwrap().view.mode.is_none(),
        "reading mini does not initialize audio"
    );
    reopened.player_snapshot("1").await.unwrap();
    let restored = reopened.mini_snapshot().unwrap();
    assert_eq!(restored.view.mode, Some(PlaybackMode::SingleLoop));
    assert_eq!(
        restored.view.mode_family,
        Some(cumulus_core::playback::mode::ModeFamily::Fm)
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn mini_empty_fm_unapplied_repeat_is_visible_after_restart_and_never_auto_applied() {
    let (dir, api, session, client) = fixture();
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let sql = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_empty_mini BEFORE UPDATE ON state WHEN NEW.owner='9' AND NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic empty FM save failure'); END;").unwrap();
    let empty = client.mini_snapshot().unwrap();
    assert!(
        client
            .mini_mode(&empty, PlaybackMode::SingleLoop)
            .await
            .is_err()
    );
    drop(client);
    let calls = api.calls.lock().unwrap().len();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened = MusicClient::with_session(session, registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    reopened.player_snapshot("1").await.unwrap();
    let current = reopened.mini_snapshot().unwrap();
    assert_eq!(current.view.mode, Some(PlaybackMode::Sequential));
    assert_eq!(
        current.view.fm_mode_pending.as_ref().unwrap().mode,
        PlaybackMode::SingleLoop
    );
    assert!(current.view.item.is_none());
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    sql.execute_batch("DROP TRIGGER reject_empty_mini;")
        .unwrap();
    let applied = reopened
        .mini_mode(&current, PlaybackMode::SingleLoop)
        .await
        .unwrap();
    assert!(applied.fm && applied.play_id.is_none());
    assert!(
        reopened
            .mini_snapshot()
            .unwrap()
            .view
            .fm_mode_pending
            .is_none()
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}
