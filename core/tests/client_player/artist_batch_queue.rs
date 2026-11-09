use super::*;
use cumulus_core::playback::{PlaybackMode, QueueEntry, QueueSnapshot, controller::PlaybackStatus};
use std::time::Duration;

async fn page(client: &MusicClient, offset: usize) -> CatalogSelectionPage {
    let query = CatalogQuery::ArtistSongs {
        id: "12".into(),
        offset,
        limit: 20,
    };
    let saved = client
        .catalog_page("1", query.clone(), true)
        .await
        .unwrap()
        .value
        .unwrap();
    CatalogSelectionPage {
        query,
        revision: saved.revision,
    }
}

fn selected(pages: Vec<CatalogSelectionPage>, ids: &[&str], revision: &str) -> PlayerAction {
    PlayerAction::QueueArtistSongsBatch {
        pages,
        ids: ids.iter().map(|id| (*id).into()).collect(),
        quality: Quality::Standard,
        expected_queue_revision: revision.into(),
    }
}

async fn displayed(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.track.id)
        .collect()
}

#[tokio::test]
async fn selected_artist_queue_uses_two_displayed_pages_and_keeps_the_exact_paused_current() {
    let (_dir, _, _, client, _) = fixture();
    let pages = vec![page(&client, 0).await, page(&client, 20).await];
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["91", "3", "2", "92"].map(str::to_owned).into(),
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 18.0 })
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    let unchanged = |view: &PlayerView| {
        assert_eq!(view.state, PlaybackStatus::Paused);
        assert_eq!(view.play_id, before.play_id);
        assert_eq!(view.entry_id, before.entry_id);
        assert_eq!(view.position, before.position);
    };
    for (scope, ids, revision) in [
        (
            pages.clone(),
            vec!["1", "999"],
            before.queue_revision.as_str(),
        ),
        (
            pages.clone(),
            vec!["1", "1"],
            before.queue_revision.as_str(),
        ),
        (pages.clone(), vec![], before.queue_revision.as_str()),
        (
            vec![pages[1].clone()],
            vec!["21"],
            before.queue_revision.as_str(),
        ),
        (pages.clone(), vec!["21"], "0"),
    ] {
        assert!(
            client
                .player_action("1", selected(scope, &ids, revision))
                .await
                .is_err()
        );
        assert_eq!(
            client.player_queue("1").await.unwrap().revision,
            queue.revision
        );
        unchanged(&client.player_snapshot("1").await.unwrap());
    }
    assert!(
        client
            .player_action(
                "0",
                selected(pages.clone(), &["21"], &before.queue_revision)
            )
            .await
            .is_err()
    );
    let added = client
        .player_action(
            "1",
            selected(pages, &["22", "3", "2"], &before.queue_revision),
        )
        .await
        .unwrap();
    unchanged(&added);
    assert_eq!(displayed(&client).await, ["91", "2", "3", "22", "92"]);
    let after = client.player_queue("1").await.unwrap();
    assert_eq!(after.current, queue.current);
    assert_eq!(after.tracks[1].entry_id, queue.tracks[2].entry_id);
    assert_eq!(after.tracks[2].entry_id, queue.tracks[1].entry_id);
}

#[tokio::test]
async fn selected_artist_queue_rejects_a_page_replaced_while_selected_song_details_are_held() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _, client, _) = fixture_with_device(device.clone());
    let initial = page(&client, 0).await;
    let before = client.player_snapshot("1").await.unwrap();
    api.hold_id.store(1, Ordering::SeqCst);
    let command = selected(vec![initial.clone()], &["3", "1"], &before.queue_revision);
    let next = client.clone();
    let pending = tokio::spawn(async move { next.player_action("1", command).await });
    tokio::time::timeout(Duration::from_secs(2), api.started.notified())
        .await
        .unwrap();
    api.artist_first_replacement.store(61, Ordering::SeqCst);
    let replaced = page(&client, 0).await;
    assert_ne!(initial.revision, replaced.revision);
    api.hold_id.store(0, Ordering::SeqCst);
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert_eq!(device.state.lock().unwrap().loads, 0);
    assert!(displayed(&client).await.is_empty());
    let recovered = client
        .player_action(
            "1",
            selected(vec![replaced], &["3", "2"], &before.queue_revision),
        )
        .await
        .unwrap();
    assert_eq!(recovered.state, PlaybackStatus::Playing);
    assert_eq!(recovered.track.unwrap().id, "2");
    assert_eq!(displayed(&client).await, ["2", "3"]);
}

#[tokio::test]
async fn selected_artist_queue_does_not_start_a_nonempty_unselected_queue() {
    let device = Arc::new(support::Device::default());
    let (dir, _, _, client, _) = fixture_with_device(device.clone());
    let pages = vec![page(&client, 0).await];
    let seeded = QueueSnapshot {
        items: vec![QueueEntry {
            entry_id: "unselected-catalog-90".into(),
            resource: json!({"id":"90","title":"未选中的歌曲"}),
            source: Value::Null,
        }],
        order: vec!["unselected-catalog-90".into()],
        current: None,
        ..Default::default()
    };
    cumulus_core::storage::StateStore::open(dir.path())
        .unwrap()
        .put(
            "9",
            "playback",
            "queue",
            &serde_json::to_value(seeded).unwrap(),
        )
        .unwrap();
    let before = client.player_snapshot("1").await.unwrap();
    assert_eq!(before.queue_len, 1);
    assert!(before.entry_id.is_none());
    let added = client
        .player_action("1", selected(pages, &["7", "2"], &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Idle);
    assert!(added.entry_id.is_none() && added.play_id.is_none());
    assert_eq!(displayed(&client).await, ["2", "7", "90"]);
    assert_eq!(device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn selected_artist_queue_sets_first_inserted_as_the_next_random_song() {
    let (_dir, _, _, client, _) = fixture();
    let pages = vec![page(&client, 0).await];
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["91", "3", "92"].map(str::to_owned).into(),
                index: 1,
                quality: Quality::Standard,
            },
        )
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
        .player_action("1", selected(pages, &["4", "2"], &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Paused);
    assert_eq!(added.play_id, before.play_id);
    assert_eq!(added.position, before.position);
    assert_eq!(displayed(&client).await, ["91", "3", "2", "4", "92"]);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "2");
}

#[tokio::test]
async fn selected_artist_queue_does_not_assume_a_known_blocked_song_is_playable() {
    let (dir, api, _, client, _) = fixture();
    let pages = vec![page(&client, 0).await];
    let before = client.player_snapshot("1").await.unwrap();
    api.artist_batch_blocked_song.store(2, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", selected(pages.clone(), &["2"], &before.queue_revision))
            .await,
        Err(Error::State("artist has no online tracks"))
    ));
    assert!(displayed(&client).await.is_empty());
    let added = client
        .player_action("1", selected(pages, &["3", "2"], &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Playing);
    assert_eq!(added.track.unwrap().id, "3");
    assert_eq!(displayed(&client).await, ["3"]);
    let stored = cumulus_core::storage::StateStore::open(dir.path())
        .unwrap()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(stored["items"][0]["source"]["kind"], "artist");
    assert_eq!(stored["items"][0]["source"]["id"], "12");
}
