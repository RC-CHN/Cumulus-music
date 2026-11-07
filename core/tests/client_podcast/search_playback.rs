use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

fn seed_search(api: &Backend) {
    let mut allowed = radio();
    allowed["categoryId"] = json!(2);
    let denied = json!({"id":8,"name":"Unlisted","underShelf":true});
    let resource = |base: Value| json!({"type":"voicelist","baseInfo":base});
    *api.search_playback_response.lock().unwrap() = json!({
        "code":200,"data":{"blocks":[
            {"blockCode":"search_block_best_match","resources":[resource(allowed.clone())]},
            {"blockCode":"search_block_voicelist","resources":[resource(allowed.clone()),resource(denied)]},
            {"blockCode":"unrelated","resources":[resource(json!({"id":22,"name":"Foreign section"}))]}
        ]}
    });
}

async fn receipts(client: &MusicClient) -> (String, String) {
    let data = client
        .search_results(
            "1",
            SearchResultQuery {
                keyword: "synthetic search podcast".into(),
                category: SearchCategory::All,
                offset: 0,
                limit: 10,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap();
    let value = serde_json::to_value(data.value).unwrap();
    let sections = &value["sections"];
    assert!(sections[1]["items"][1].get("playbackReceipt").is_none());
    assert!(sections[2]["items"][0].get("playbackReceipt").is_none());
    assert!(!value.to_string().contains("categoryId"));
    (
        sections[0]["items"][0]["playbackReceipt"]
            .as_str()
            .unwrap()
            .to_owned(),
        sections[1]["items"][0]["playbackReceipt"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}

async fn prepared(
    client: &MusicClient,
    selection: PlaySelection,
) -> cumulus_core::Result<PlayerView> {
    let current = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &current.queue_revision, current.play_id.as_deref())?;
    client
        .commit_prepared_selection("1", &ticket, selection)
        .await
}

fn select(receipt: &str, id: &str) -> PlaySelection {
    PlaySelection::SearchPodcastCard {
        receipt: receipt.into(),
        id: id.into(),
    }
}

fn store_checkpoint(root: &std::path::Path) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let store = registry.store();
    store
        .put("9", "podcast-radio-order-v1", "7", &json!(false))
        .unwrap();
    store
        .put(
            "9",
            "podcast-latest-voice-v1",
            "7",
            &json!({
                "id":"1","mainTrackId":"1001","name":"Episode 1","time":19,"duration":120
            }),
        )
        .unwrap();
}

#[tokio::test]
async fn search_podcast_overlays_use_saved_order_across_250_and_reuse_selected_decoder() {
    let (dir, api, _session, client, device) = fixture_with_device();
    seed_search(&api);
    api.count.store(251, Ordering::SeqCst);
    api.search_playback_revision.store(1, Ordering::SeqCst);
    store_checkpoint(dir.path());
    let (best, regular) = receipts(&client).await;
    assert_ne!(best, regular);
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    for id in ["8", "22"] {
        assert!(matches!(
            prepared(&client, select(&best, id)).await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(
        client
            .play_selection("1", select(&best, "7"))
            .await
            .is_err()
    );
    assert!(api.calls.lock().unwrap().is_empty());
    let started = prepared(&client, select(&best, "7")).await.unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Sequential);
    assert_eq!(started.position, Some(19.0));
    assert_eq!(started.track.unwrap().voice_id.as_deref(), Some("1"));
    assert_eq!(started.queue_len, 251);
    assert!(matches!(started.origin, Some(PlayerOrigin::Podcast { ref id }) if id == "7"));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue.tracks.first().unwrap().track.voice_id.as_deref(),
        Some("251")
    );
    assert_eq!(
        queue.tracks.last().unwrap().track.voice_id.as_deref(),
        Some("1")
    );
    let calls = api.calls.lock().unwrap().clone();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/v6/dj/program/byradio")
            .map(|(_, args)| args.clone())
            .collect::<Vec<_>>(),
        [
            json!({"radioId":"7","offset":0,"limit":250,"asc":false}),
            json!({"radioId":"7","offset":250,"limit":250,"asc":false})
        ]
    );
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/dj/play/url/check")
            .map(|(_, p)| &p["voiceId"])
            .collect::<Vec<_>>(),
        [&json!("1")]
    );
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/song/enhance/player/url/v1")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/v6/dj/program/byradio",
            "/api/v6/dj/program/byradio",
            "/api/dj/program/detail",
            "/api/dj/play/url/check",
            "/api/song/enhance/player/url/v1"
        ]
    );
    assert_eq!(calls[2].1, json!({"id":"1"}));
    for (receipt, state) in [
        (&regular, PlaybackStatus::Paused),
        (&best, PlaybackStatus::Playing),
    ] {
        let changed = prepared(&client, select(receipt, "7")).await.unwrap();
        assert_eq!(changed.state, state);
        assert_eq!(changed.play_id, started.play_id);
        assert_eq!(changed.queue_revision, started.queue_revision);
        assert_eq!(changed.position, started.position);
    }
    assert_eq!(*api.calls.lock().unwrap(), calls);
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn denied_or_stale_search_podcast_does_not_replace_old_queue() {
    let (dir, api, _session, client, device) = fixture_with_device();
    seed_search(&api);
    api.count.store(2, Ordering::SeqCst);
    api.search_playback_revision.store(1, Ordering::SeqCst);
    store_checkpoint(dir.path());
    let (best, _) = receipts(&client).await;
    prepared(&client, PlaySelection::Voice { id: "2".into() })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let previous = client.player_snapshot("1").await.unwrap();
    let same = |view: PlayerView| {
        assert_eq!(view.play_id, previous.play_id);
        assert_eq!(view.queue_revision, previous.queue_revision);
        assert_eq!(view.position, previous.position);
        assert_eq!(view.state, PlaybackStatus::Paused);
    };
    api.search_playback_deny.store(1, Ordering::SeqCst);
    assert!(prepared(&client, select(&best, "7")).await.is_err());
    same(client.player_snapshot("1").await.unwrap());
    api.search_playback_deny.store(0, Ordering::SeqCst);
    api.search_playback_hold.store(true, Ordering::SeqCst);
    let client = Arc::new(client);
    let pending = tokio::spawn({
        let client = client.clone();
        let selection = select(&best, "7");
        async move { prepared(&client, selection).await }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        api.search_playback_started.notified(),
    )
    .await
    .unwrap();
    let held_calls = api.calls.lock().unwrap().len();
    api.search_playback_revision.store(2, Ordering::SeqCst);
    api.search_playback_release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(api.calls.lock().unwrap().len(), held_calls);
    same(client.player_snapshot("1").await.unwrap());
    let calls = api.calls.lock().unwrap().len();
    assert!(matches!(
        prepared(&client, select(&best, "7")).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    assert_eq!(device.state.lock().unwrap().loads, 1);
}
