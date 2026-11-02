use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn add(revision: &str) -> PlayerAction {
    PlayerAction::QueueAlbum {
        id: "7".into(),
        quality: Quality::Lossless,
        expected_queue_revision: revision.into(),
    }
}
#[tokio::test]
async fn album_insertion_preserves_paused_transport_deduplicates_and_persists_album_source() {
    let (dir, api, _, client, _) = fixture();
    *api.album_tracks.lock().unwrap() =
        Some(vec![json!({"id":2}), json!({"id":3}), json!({"id":2})]);
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "4".into()],
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
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let after = client
        .player_action("1", add(&before.queue_revision))
        .await
        .unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(
        (after.play_id, after.entry_id, after.position),
        (before.play_id, before.entry_id, before.position)
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|entry| entry.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "4"]
    );
    let persisted = cumulus_core::playback::PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(dir.path()).unwrap(),
        )),
        "9",
    )
    .unwrap()
    .snapshot();
    let inserted = persisted
        .items
        .iter()
        .find(|entry| entry.resource["id"] == "3")
        .unwrap();
    assert_eq!(inserted.source["kind"], "album");
    assert_eq!(inserted.source["id"], "7");
    assert_eq!(inserted.resource["trial_mode"], -1);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "2");
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "3");
    assert_eq!(api.trial.lock().unwrap().as_ref().unwrap(), &json!(-1));
}
#[tokio::test]
async fn empty_queue_starts_album_once_and_stale_or_empty_insertions_preserve_queue() {
    let (_dir, api, _, client, _) = fixture();
    *api.album_tracks.lock().unwrap() =
        Some(vec![json!({"id":2}), json!({"id":2}), json!({"id":3})]);
    let initial = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    let added = client
        .player_action("1", add(&initial.queue_revision.to_string()))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Playing);
    assert_eq!(added.track.unwrap().id, "2");
    assert_eq!(added.queue_len, 2);
    assert!(matches!(
        client
            .player_action("1", add(&initial.queue_revision.to_string()))
            .await,
        Err(Error::QueueChanged)
    ));
    *api.album_tracks.lock().unwrap() = Some(vec![]);
    assert!(matches!(
        client.player_action("1", add(&added.queue_revision)).await,
        Err(Error::State("album has no online tracks"))
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 2);
}
#[tokio::test]
async fn delayed_album_cannot_override_new_transport_or_account() {
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
