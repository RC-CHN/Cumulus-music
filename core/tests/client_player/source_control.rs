use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn pause(view: &PlayerView, paused: bool) -> PlayerAction {
    PlayerAction::PauseFor {
        play_id: view.play_id.clone().unwrap(),
        expected_queue_revision: view.queue_revision.clone(),
        paused,
    }
}

#[tokio::test]
async fn failed_native_resume_can_be_explicitly_retried_without_rebuilding_the_source() {
    let (_dir, api, _, client, _) = fixture();
    let device = Arc::new(support::Device::default());
    let engine_device = device.clone();
    let client = Arc::try_unwrap(client)
        .ok()
        .expect("test owns the unused client")
        .with_audio_factory(move || Ok(support::audio_engine(engine_device.clone())));
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 19.0 })
        .await
        .unwrap();
    let paused = client
        .player_action("1", pause(&playing, true))
        .await
        .unwrap();
    let calls = api.calls.lock().unwrap().len();
    device.fail_unpause.store(true, Ordering::SeqCst);
    assert!(
        client
            .player_action("1", pause(&paused, false))
            .await
            .is_err()
    );
    let failed = client.player_snapshot("1").await.unwrap();
    assert_eq!(failed.state, PlaybackStatus::Paused);
    assert_eq!(failed.play_id, paused.play_id);
    assert_eq!(failed.queue_revision, paused.queue_revision);
    device.fail_unpause.store(false, Ordering::SeqCst);
    let resumed = client
        .player_action("1", pause(&paused, false))
        .await
        .unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.queue_revision, paused.queue_revision);
    assert!(resumed.position.unwrap() >= 19.0);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn source_pause_and_resume_preserve_nonfirst_track_queue_clock_and_requests() {
    let (_dir, api, _, client, engines) = fixture();
    *api.playlist_tracks.lock().unwrap() = Some((1..=3).map(|id| json!({"id":id})).collect());
    let playing = client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: Some("2".into()),
                revision: None,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 45.0 })
        .await
        .unwrap();
    let before = api.calls.lock().unwrap().len();
    let queue = client.player_queue("1").await.unwrap();
    for paused in [true, false, true, false] {
        let next = client
            .player_action("1", pause(&playing, paused))
            .await
            .unwrap();
        assert_eq!(next.play_id, playing.play_id);
        assert_eq!(next.entry_id, playing.entry_id);
        assert_eq!(next.queue_revision, playing.queue_revision);
        assert_eq!(next.track.unwrap().id, "2");
        assert!(next.position.unwrap() >= 45.0);
        assert_eq!(
            next.state,
            if paused {
                PlaybackStatus::Paused
            } else {
                PlaybackStatus::Playing
            }
        );
    }
    assert_eq!(
        serde_json::to_value(client.player_queue("1").await.unwrap()).unwrap(),
        serde_json::to_value(queue).unwrap()
    );
    assert_eq!(api.calls.lock().unwrap().len(), before);
    assert_eq!(engines.load(Ordering::SeqCst), 1);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn old_playback_malformed_queue_and_stopped_source_cannot_control_current_audio() {
    let (_dir, api, _, client, _) = fixture();
    let first = client.play_selection("1", songs("1")).await.unwrap();
    let current = client.play_selection("1", songs("2")).await.unwrap();
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.player_action("1", pause(&first, true)).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .player_action(
                "1",
                PlayerAction::PauseFor {
                    play_id: current.play_id.clone().unwrap(),
                    expected_queue_revision: "00".into(),
                    paused: true,
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    let unchanged = client.player_snapshot("1").await.unwrap();
    assert_eq!(unchanged.play_id, current.play_id);
    assert_eq!(unchanged.state, PlaybackStatus::Playing);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert!(
        client
            .player_action("1", pause(&current, false))
            .await
            .is_err()
    );
    assert!(matches!(
        client.player_action("0", pause(&current, false)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn changed_queue_rejects_old_source_control_even_when_play_id_is_retained() {
    let (_dir, _, _, client, _) = fixture();
    let before = client.play_selection("1", songs("1")).await.unwrap();
    let after = client
        .player_action(
            "1",
            PlayerAction::QueuePlaylist {
                id: "7".into(),
                quality: Quality::Lossless,
                expected_queue_revision: before.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(before.play_id, after.play_id);
    assert_ne!(before.queue_revision, after.queue_revision);
    assert!(matches!(
        client.player_action("1", pause(&before, true)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client.player_snapshot("1").await.unwrap().state,
        PlaybackStatus::Playing
    );
    assert_eq!(
        client
            .player_action("1", pause(&after, true))
            .await
            .unwrap()
            .state,
        PlaybackStatus::Paused
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn rejected_old_source_control_does_not_cancel_a_newer_pending_selection() {
    let (_dir, api, _, client, _) = fixture();
    let first = client.play_selection("1", songs("1")).await.unwrap();
    client.play_selection("1", songs("2")).await.unwrap();
    api.hold_url_id.store(3, Ordering::SeqCst);
    let pending_client = client.clone();
    let pending = tokio::spawn(async move { pending_client.play_selection("1", songs("3")).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        api.url_started.notified(),
    )
    .await
    .unwrap();
    assert!(matches!(
        client.player_action("1", pause(&first, true)).await,
        Err(Error::StaleResponse)
    ));
    api.url_release.notify_one();
    let loaded = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(loaded.track.unwrap().id, "3");
    assert_eq!(loaded.state, PlaybackStatus::Playing);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn old_seek_does_not_seek_the_replacement_and_current_seek_preserves_playback() {
    let (_dir, api, _, client, _) = fixture();
    let first = client.play_selection("1", songs("1")).await.unwrap();
    let second = client.play_selection("1", songs("2")).await.unwrap();
    let request = |view: &PlayerView| PlayerAction::SeekFor {
        play_id: view.play_id.clone().unwrap(),
        expected_queue_revision: view.queue_revision.clone(),
        seconds: 35.0,
    };
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.player_action("1", request(&first)).await,
        Err(Error::StaleResponse)
    ));
    let untouched = client.player_snapshot("1").await.unwrap();
    assert_eq!(untouched.play_id, second.play_id);
    assert!(untouched.position.unwrap() < 5.0);

    let seek = client.player_action("1", request(&second)).await.unwrap();
    assert_eq!(seek.play_id, second.play_id);
    assert_eq!(seek.queue_revision, second.queue_revision);
    assert_eq!(seek.state, PlaybackStatus::Playing);
    assert!(seek.position.unwrap() >= 35.0);
    assert_eq!(api.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn pending_transport_pauses_the_same_search_load_before_audio_can_start() {
    use std::time::Duration;
    let device = Arc::new(support::Device::default());
    device.hold_load.store(true, Ordering::SeqCst);
    let (_dir, api, _, client, _) = fixture_with_device(device.clone());
    api.hold_url_id.store(1, Ordering::SeqCst);
    let loading_client = client.clone();
    let load = tokio::spawn(async move {
        loading_client
            .play_selection(
                "1",
                PlaySelection::SearchResultTrack {
                    id: "1".into(),
                    quality: Quality::Standard,
                    toggle_current: false,
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), api.url_started.notified())
        .await
        .unwrap();
    let preparing = client.player_snapshot("1").await.unwrap();
    assert_eq!(preparing.state, PlaybackStatus::Preparing);
    assert_eq!(preparing.pending_paused, Some(false));
    let mut stale = preparing.clone();
    stale.play_id = Some("older-source".into());
    assert!(matches!(
        client.player_action("1", pause(&stale, true)).await,
        Err(Error::StaleResponse)
    ));
    let preparing_paused = client
        .player_action("1", pause(&preparing, true))
        .await
        .unwrap();
    assert_eq!(preparing_paused.pending_paused, Some(true));
    assert_eq!(preparing_paused.play_id, preparing.play_id);
    api.url_release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), device.started.notified())
        .await
        .unwrap();
    let decoding = client.player_snapshot("1").await.unwrap();
    assert_eq!(decoding.state, PlaybackStatus::Loading);
    assert_eq!(decoding.pending_paused, Some(true));
    for paused in [false, true] {
        let staged = client
            .player_action("1", pause(&decoding, paused))
            .await
            .unwrap();
        assert_eq!(staged.state, PlaybackStatus::Loading);
        assert_eq!(staged.pending_paused, Some(paused));
        assert_eq!(staged.play_id, preparing.play_id);
    }
    assert!(device.state.lock().unwrap().pauses.is_empty());
    device.hold_load.store(false, Ordering::SeqCst);
    let loaded = tokio::time::timeout(Duration::from_secs(5), load)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(loaded.state, PlaybackStatus::Paused);
    assert_eq!(loaded.pending_paused, None);
    assert_eq!(loaded.play_id, preparing.play_id);
    assert_eq!(loaded.queue_revision, preparing.queue_revision);
    assert_eq!(device.state.lock().unwrap().pauses, [true]);
    let resumed = client
        .player_action("1", pause(&loaded, false))
        .await
        .unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, preparing.play_id);
    let native = device.state.lock().unwrap();
    assert_eq!(native.loads, 1);
    assert_eq!(native.pauses, [true, false]);
}

#[tokio::test]
async fn seek_during_decode_uses_the_latest_position_without_replacing_the_load() {
    for (decoded_duration, expected, reset, trial) in [
        (120.0, 45.0, false, false),
        (20.0, 0.0, true, false),
        (20.0, 35.0, false, true),
    ] {
        let device = Arc::new(support::Device::default());
        device.state.lock().unwrap().duration = decoded_duration;
        device.hold_load.store(true, Ordering::SeqCst);
        let (_dir, api, _, client, _) = fixture_with_device(device.clone());
        api.quality_trial_source.store(trial, Ordering::SeqCst);
        let loading_client = client.clone();
        let load = tokio::spawn(async move {
            // Search's retained row owns the room policy gate during decode.
            loading_client
                .play_selection(
                    "1",
                    PlaySelection::SearchResultTrack {
                        id: "1".into(),
                        quality: Quality::Standard,
                        toggle_current: false,
                    },
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), device.started.notified())
            .await
            .unwrap();
        let loading = client.player_snapshot("1").await.unwrap();
        assert_eq!(loading.state, PlaybackStatus::Loading);
        let timeline = loading.timeline.as_ref().unwrap();
        assert_eq!(timeline.start, if trial { 20.0 } else { 0.0 });
        assert_eq!(timeline.end, Some(if trial { 40.0 } else { 120.0 }));
        let seek = |seconds, revision| PlayerAction::SeekFor {
            play_id: loading.play_id.clone().unwrap(),
            expected_queue_revision: revision,
            seconds,
        };
        assert!(matches!(
            client.player_action("1", seek(75.0, "0".into())).await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            client
                .player_action(
                    "1",
                    seek(
                        if trial { 15.0 } else { 125.0 },
                        loading.queue_revision.clone()
                    )
                )
                .await,
            Err(Error::InvalidInput(_))
        ));
        for position in if trial { [25.0, 35.0] } else { [15.0, 45.0] } {
            let staged = client
                .player_action("1", seek(position, loading.queue_revision.clone()))
                .await
                .unwrap();
            assert_eq!(staged.play_id, loading.play_id);
            assert_eq!(staged.state, PlaybackStatus::Loading);
            assert_eq!(staged.position, Some(position));
        }
        assert_eq!(device.state.lock().unwrap().loads, 0);
        device.hold_load.store(false, Ordering::SeqCst);
        let playing = tokio::time::timeout(std::time::Duration::from_secs(5), load)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(playing.play_id, loading.play_id);
        assert_eq!(playing.queue_revision, loading.queue_revision);
        assert_eq!(playing.state, PlaybackStatus::Playing);
        assert_eq!(playing.position, Some(expected));
        assert_eq!(playing.position_reset, reset);
        assert_eq!(
            playing.timeline.unwrap().end,
            Some(if trial { 40.0 } else { decoded_duration })
        );
        {
            let native = device.state.lock().unwrap();
            assert_eq!(native.loads, 1);
            assert_eq!(
                native.view.position,
                Some(if trial { 15.0 } else { expected })
            );
        }
        assert_eq!(
            api.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.as_str() == "/api/song/enhance/player/url/v1")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn failed_source_refresh_is_fenced_to_the_error_entry() {
    let (_dir, api, _, client, _) = fixture();
    let device = Arc::new(support::Device::default());
    let engine_device = device.clone();
    let client = Arc::try_unwrap(client)
        .ok()
        .expect("test owns the unused client")
        .with_audio_factory(move || Ok(support::audio_engine(engine_device.clone())));
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let request = |view: &PlayerView| PlayerAction::RefreshFor {
        play_id: view.play_id.clone().unwrap(),
        expected_queue_revision: view.queue_revision.clone(),
    };
    assert!(client.player_action("1", request(&playing)).await.is_err());
    client
        .player_action("1", pause(&playing, true))
        .await
        .unwrap();
    device.load_failure_code.store(-15, Ordering::SeqCst);
    assert!(
        client
            .player_action("1", PlayerAction::Refresh)
            .await
            .is_err()
    );
    let failed = client.player_snapshot("1").await.unwrap();
    assert_eq!(failed.state, PlaybackStatus::Error);
    device.load_failure_code.store(0, Ordering::SeqCst);
    let retried = client.player_action("1", request(&failed)).await.unwrap();
    assert_eq!(retried.state, PlaybackStatus::Paused);
    let calls = api.calls.lock().unwrap().len();
    assert!(client.player_action("1", request(&failed)).await.is_err());
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}
