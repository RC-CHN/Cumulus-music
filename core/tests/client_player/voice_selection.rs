use super::*;

#[tokio::test]
async fn single_voice_resolves_its_main_media_and_preserves_program_identity() {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client
        .play_selection("1", PlaySelection::Voice { id: "81".into() })
        .await
        .unwrap();
    assert_eq!(playing.track.as_ref().unwrap().id, "181");
    assert_eq!(
        playing.track.as_ref().unwrap().voice_id.as_deref(),
        Some("81")
    );
    assert_eq!(playing.resource_kind, Some(PlayerResourceKind::Voice));
    assert_eq!(playing.requested_quality.as_deref(), Some("standard"));
    let before = api.calls.lock().unwrap().clone();
    assert!(before.iter().any(|s| s == "/api/dj/play/url/check"));
    assert!(!before.iter().any(|s| s == "/api/v3/song/detail"));
    let paused = client
        .player_action(
            "1",
            PlayerAction::PauseFor {
                play_id: playing.play_id.clone().unwrap(),
                expected_queue_revision: playing.queue_revision.clone(),
                paused: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.entry_id, playing.entry_id);
    assert_eq!(paused.queue_revision, playing.queue_revision);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn late_voice_selection_cannot_replace_a_newer_song_and_denied_program_cannot_play() {
    let (_dir, api, _session, client, _) = fixture();
    api.hold_id.store(81, Ordering::SeqCst);
    let other = client.clone();
    let pending = tokio::spawn(async move {
        other
            .play_selection("1", PlaySelection::Voice { id: "81".into() })
            .await
    });
    api.started.notified().await;
    let current = client.play_selection("1", songs("2")).await.unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        client.player_snapshot("1").await.unwrap().play_id,
        current.play_id
    );
    api.hold_id.store(0, Ordering::SeqCst);
    api.deny_voice.store(true, Ordering::SeqCst);
    assert!(
        client
            .play_selection("1", PlaySelection::Voice { id: "81".into() })
            .await
            .is_err()
    );
    assert!(
        client.player_snapshot("1").await.unwrap().state
            != cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}
