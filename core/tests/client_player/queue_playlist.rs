use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

fn add(revision: &str) -> PlayerAction {
    PlayerAction::QueuePlaylist {
        id: "2".into(),
        quality: Quality::Lossless,
        expected_queue_revision: revision.into(),
    }
}

#[tokio::test]
async fn whole_playlist_preserves_paused_current_deduplicates_and_persists_explicit_next() {
    let (dir, _, _, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1000".into(), "2".into(), "2000".into()],
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
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let added = client
        .player_action("1", add(&before.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.play_id, before.play_id);
    assert_eq!(added.entry_id, before.entry_id);
    assert_eq!(added.position, before.position);
    assert_eq!(added.state, PlaybackStatus::Paused);
    let queue = client.player_queue("1").await.unwrap();
    let expected: Vec<_> = std::iter::once("1000".to_string())
        .chain((1..=205).map(|i| i.to_string()))
        .chain(["2000".to_string()])
        .collect();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|t| t.track.id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let encoded = serde_json::to_string(&queue).unwrap();
    assert!(!encoded.contains("SECRET"));
    let persisted = cumulus_core::playback::PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(dir.path()).unwrap(),
        )),
        "9",
    )
    .unwrap()
    .snapshot();
    assert_eq!(persisted.items.len(), 207);
    let current = persisted
        .order
        .iter()
        .position(|e| Some(e) == persisted.current.as_ref())
        .unwrap();
    let next = &persisted.order[current + 1];
    assert_eq!(
        persisted
            .items
            .iter()
            .find(|e| &e.entry_id == next)
            .unwrap()
            .resource["id"],
        "1"
    );
    let played = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(played.track.unwrap().id, "1");
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let repeated = client
        .player_action("1", add(&played.queue_revision))
        .await
        .unwrap();
    assert_eq!(repeated.queue_len, 207);
    assert_eq!(repeated.play_id, played.play_id);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "2");
}

#[tokio::test]
async fn empty_queue_starts_once_and_empty_or_local_only_playlist_does_not_change_queue() {
    let (_dir, api, _, client, _) = fixture();
    let initial = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    *api.playlist_tracks.lock().unwrap() = Some(vec![
        json!({"id":1,"v":-1}),
        json!({"id":2}),
        json!({"id":2}),
        json!({"id":3}),
    ]);
    let added = client
        .player_action("1", add(&initial.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Playing);
    assert_eq!(added.track.unwrap().id, "2");
    assert_eq!(added.queue_len, 2);
    assert!(matches!(
        client
            .player_action("1", add(&initial.queue_revision))
            .await,
        Err(Error::QueueChanged)
    ));
    for tracks in [vec![], vec![json!({"id":4,"v":-1})]] {
        *api.playlist_tracks.lock().unwrap() = Some(tracks);
        assert!(matches!(
            client.player_action("1", add(&added.queue_revision)).await,
            Err(Error::State("playlist has no online tracks"))
        ));
        assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 2);
    }
}

#[tokio::test]
async fn delayed_playlist_cannot_override_new_transport_or_account() {
    let (_dir, api, session, client, _) = fixture();
    let before = client.play_selection("1", songs("1000")).await.unwrap();
    api.hold_id.store(1, Ordering::SeqCst);
    let c = client.clone();
    let revision = before.queue_revision.clone();
    let pending = tokio::spawn(async move { c.player_action("1", add(&revision)).await });
    api.started.notified().await;
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 1);
    let c = client.clone();
    let pending =
        tokio::spawn(async move { c.player_action("1", add(&before.queue_revision)).await });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(client.player_queue("2").await.unwrap().tracks.is_empty());
}
