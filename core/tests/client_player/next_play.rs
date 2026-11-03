use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

fn next(id: &str, revision: &str) -> PlayerAction {
    PlayerAction::NextPlay {
        id: id.into(),
        quality: Quality::Lossless,
        expected_queue_revision: revision.into(),
    }
}

#[tokio::test]
async fn empty_next_starts_audio_and_retry_with_old_revision_never_duplicates() {
    let (_dir, _, _, client, _) = fixture();
    let initial = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    let playing = client
        .player_action("1", next("11", &initial.queue_revision))
        .await
        .unwrap();
    assert_eq!(playing.track.unwrap().id, "11");
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert!(matches!(
        client
            .player_action("1", next("11", &initial.queue_revision))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 1);
}

#[tokio::test]
async fn inserts_after_current_preserves_paused_audio_and_moves_existing_without_duplicates() {
    let (_dir, _, _, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 0,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 25.0 })
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let updated = client
        .player_action("1", next("3", &paused.queue_revision))
        .await
        .unwrap();
    assert_eq!(updated.state, PlaybackStatus::Paused);
    assert_eq!(updated.play_id, paused.play_id);
    assert_eq!(updated.entry_id, paused.entry_id);
    assert_eq!(updated.position, paused.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|s| s.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "3", "2"]
    );
    let inserted = client
        .player_action("1", next("4", &updated.queue_revision))
        .await
        .unwrap();
    assert_eq!(inserted.play_id, updated.play_id);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|s| s.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "4", "3", "2"]
    );
    let same = client
        .player_action("1", next("1", &inserted.queue_revision))
        .await
        .unwrap();
    assert_eq!(same.queue_revision, inserted.queue_revision);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "4");
}

#[tokio::test]
async fn random_mode_honors_explicit_next_and_persists_queue_order() {
    let (dir, _, _, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 0,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    let random = client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let updated = client
        .player_action("1", next("9", &random.queue_revision))
        .await
        .unwrap();
    assert_eq!(updated.mode, PlaybackMode::Random);
    let restored = cumulus_core::playback::PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(dir.path()).unwrap(),
        )),
        "9",
    )
    .unwrap()
    .snapshot();
    assert_eq!(restored.items.len(), 4);
    let current = restored
        .order
        .iter()
        .position(|id| Some(id) == restored.current.as_ref())
        .unwrap();
    let next_id = &restored.order[current + 1];
    assert_eq!(
        restored
            .items
            .iter()
            .find(|e| &e.entry_id == next_id)
            .unwrap()
            .resource["id"],
        "9"
    );
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "9");
}

#[tokio::test]
async fn delayed_next_cannot_override_newer_transport_or_replacement_account() {
    let (_dir, api, session, client, _) = fixture();
    let initial = client.play_selection("1", songs("1")).await.unwrap();
    api.hold_id.store(7, Ordering::SeqCst);
    let c = client.clone();
    let request = tokio::spawn(async move {
        c.player_action("1", next("7", &initial.queue_revision))
            .await
    });
    api.started.notified().await;
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    api.release.notify_one();
    assert!(matches!(request.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 1);
    let initial = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    let c = client.clone();
    let request = tokio::spawn(async move {
        c.player_action("1", next("7", &initial.queue_revision))
            .await
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(request.await.unwrap(), Err(Error::StaleResponse)));
    assert!(client.player_queue("2").await.unwrap().tracks.is_empty());
}

fn search_next(id: &str, revision: &str) -> PlayerAction {
    PlayerAction::SearchNextPlay {
        id: id.into(),
        quality: Quality::Lossless,
        expected_queue_revision: revision.into(),
    }
}

#[tokio::test]
async fn search_next_starts_empty_queue_with_search_trial() {
    let (_dir, api, _, client, _) = fixture();
    let initial = client.player_snapshot("1").await.unwrap();
    let playing = client
        .player_action("1", search_next("11", &initial.queue_revision))
        .await
        .unwrap();
    assert_eq!(playing.track.unwrap().id, "11");
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 1);
}

#[tokio::test]
async fn search_next_rebinds_existing_song_preserving_queue_identity_and_uses_trial_on_advance() {
    let (dir, api, _, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3"].map(str::to_owned).to_vec(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let baseline = client.player_queue("1").await.unwrap();

    let same = client
        .player_action("1", search_next("1", &paused.queue_revision))
        .await
        .unwrap();
    assert_eq!(same.play_id, paused.play_id);
    assert_eq!(same.queue_revision, paused.queue_revision);
    let queued = client
        .player_action("1", search_next("3", &paused.queue_revision))
        .await
        .unwrap();
    assert_eq!(queued.state, PlaybackStatus::Paused);
    assert_eq!(queued.play_id, paused.play_id);
    assert_eq!(queued.position, paused.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| (row.track.id.as_str(), row.entry_id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("1", baseline.tracks[0].entry_id.as_str()),
            ("3", baseline.tracks[2].entry_id.as_str()),
            ("2", baseline.tracks[1].entry_id.as_str())
        ]
    );
    let saved = cumulus_core::playback::PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(dir.path()).unwrap(),
        )),
        "9",
    )
    .unwrap()
    .snapshot();
    assert_eq!(
        saved.items[1].source,
        json!({"kind":"songs","scene":"search","trial_mode":42})
    );
    assert_eq!(saved.items[1].resource["trial_mode"], 42);
    assert_eq!(saved.items[1].resource["level"], "standard");
    assert!(saved.items[2].resource.get("trial_mode").is_none());

    let following = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(following.track.unwrap().id, "3");
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
}
