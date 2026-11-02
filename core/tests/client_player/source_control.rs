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
