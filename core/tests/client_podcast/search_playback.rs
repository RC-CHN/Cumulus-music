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
            {"blockCode":"search_block_voicelist:unrelated","resources":[resource(json!({"id":22,"name":"Foreign section"}))]}
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

async fn row_receipt(client: &MusicClient) -> String {
    let data = client
        .search_results(
            "1",
            SearchResultQuery {
                keyword: "synthetic search podcast".into(),
                category: SearchCategory::Podcasts,
                offset: 0,
                limit: 10,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap();
    let rows = serde_json::to_value(data.value).unwrap();
    assert!(
        rows["sections"][0]["items"][1]
            .get("playbackReceipt")
            .is_none()
    );
    rows["sections"][0]["items"][0]["playbackReceipt"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn select_row(receipt: &str, gesture: SearchPodcastRowGesture) -> PlaySelection {
    PlaySelection::SearchPodcastRow {
        receipt: receipt.into(),
        id: "7".into(),
        gesture,
    }
}
fn replace_preference(client: &MusicClient, enabled: bool) {
    let prefs = client.queue_click_preferences("1").unwrap();
    client
        .queue_click_preferences_command(
            "1",
            QueueClickPreferencesCommand::Change {
                expected_revision: serde_json::to_value(prefs.value).unwrap()["revision"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                replace_queue: enabled,
            },
        )
        .unwrap();
}

#[tokio::test]
async fn classified_podcast_index_uses_the_row_lane_resume_and_exact_same_decoder() {
    let (dir, api, _session, client, device) = fixture_with_device();
    api.count.store(2, Ordering::SeqCst);
    seed_search(&api);
    store_checkpoint(dir.path());
    api.search_playback_missing_list_media
        .store(true, Ordering::SeqCst);
    let row = row_receipt(&client).await;
    let (card, _) = receipts(&client).await;
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
    for input in [
        select_row(&card, SearchPodcastRowGesture::Index),
        select(&row, "7"),
    ] {
        assert!(matches!(
            prepared(&client, input).await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(
        client
            .play_selection("1", select_row(&row, SearchPodcastRowGesture::Index))
            .await
            .is_err()
    );
    assert!(api.calls.lock().unwrap().is_empty());
    let first = prepared(&client, select_row(&row, SearchPodcastRowGesture::Index))
        .await
        .unwrap();
    assert_eq!(first.mode, PlaybackMode::Sequential);
    assert_eq!(first.track.as_ref().unwrap().voice_id.as_deref(), Some("1"));
    assert_eq!(first.position, Some(19.0));
    assert_eq!(first.queue_len, 2);
    let paused = prepared(&client, select_row(&row, SearchPodcastRowGesture::Index))
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, first.play_id);
    assert_eq!(paused.position, first.position);
    assert_eq!(paused.queue_revision, first.queue_revision);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let calls = api.calls.lock().unwrap().clone();
    assert_eq!(
        calls[0],
        (
            "/api/v6/dj/program/byradio".into(),
            json!({"radioId":"7","limit":250,"offset":0,"asc":false})
        )
    );
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/dj/play/url/check")
            .count(),
        1
    );
    assert!(!calls.iter().any(|(p, _)| matches!(
        p.as_str(),
        "/api/djradio/v3/get" | "/api/dj/around/program/v1"
    )));
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/dj/program/detail")
            .count(),
        1
    );
    api.calls.lock().unwrap().clear();
    let resumed = prepared(
        &client,
        select_row(&row, SearchPodcastRowGesture::DoubleClick),
    )
    .await
    .unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.position, paused.position);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/v6/dj/program/byradio",
            "/api/dj/program/detail",
            "/api/dj/play/url/check",
        ]
    );
}

#[tokio::test]
async fn missing_classified_podcast_checkpoint_never_guesses_another_episode() {
    let (dir, api, _session, client, device) = fixture_with_device();
    api.count.store(3, Ordering::SeqCst);
    let row = row_receipt(&client).await;
    let baseline = prepared(&client, PlaySelection::Voice { id: "5".into() })
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
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap().store().put(
        "9", "podcast-latest-voice-v1", "7",
        &json!({"id":"50","mainTrackId":"1050","name":"Missing episode","time":19,"duration":120}),
    ).unwrap();
    api.calls.lock().unwrap().clear();
    assert!(
        prepared(
            &client,
            select_row(&row, SearchPodcastRowGesture::DoubleClick)
        )
        .await
        .is_err()
    );
    let unchanged = client.player_snapshot("1").await.unwrap();
    assert_eq!(unchanged.play_id, paused.play_id);
    assert_eq!(unchanged.queue_revision, paused.queue_revision);
    replace_preference(&client, false);
    let inserted = prepared(
        &client,
        select_row(&row, SearchPodcastRowGesture::DoubleClick),
    )
    .await
    .unwrap();
    assert_eq!(inserted.state, PlaybackStatus::Paused);
    assert_eq!(inserted.play_id, paused.play_id);
    assert_eq!(inserted.position, paused.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|t| t.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["5", "1", "2", "3"]
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(baseline.play_id, paused.play_id);

    client.player_action("1", PlayerAction::Stop).await.unwrap();
    // Menu is PlayAll, independent of the double-click preference. With no
    // running decoder the same stale bookmark permits a queue-only clear.
    let idle = prepared(&client, select_row(&row, SearchPodcastRowGesture::Menu))
        .await
        .unwrap();
    assert_eq!(idle.state, PlaybackStatus::Idle);
    assert!(idle.play_id.is_none());
    assert_eq!(idle.mode, PlaybackMode::Sequential);
    assert_eq!(idle.queue_len, 3);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let calls = api.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            (
                "/api/v6/dj/program/byradio".into(),
                json!({"radioId":"7","limit":250,"offset":0})
            );
            3
        ]
    );
}

#[tokio::test]
async fn classified_podcast_no_checkpoint_chooses_the_batch_and_preserves_random_mode() {
    let (_dir, api, _session, client, _device) = fixture_with_device();
    api.count.store(3, Ordering::SeqCst);
    api.search_playback_curated.store(true, Ordering::SeqCst);
    let row = row_receipt(&client).await;
    prepared(&client, PlaySelection::Voice { id: "5".into() })
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
    replace_preference(&client, false);
    let started = prepared(
        &client,
        select_row(&row, SearchPodcastRowGesture::DoubleClick),
    )
    .await
    .unwrap();
    assert_eq!(started.mode, PlaybackMode::Random);
    assert!(matches!(started.origin, Some(PlayerOrigin::Podcast { ref id }) if id == "7"));
    let view = serde_json::to_value(&started).unwrap();
    assert_eq!(view["voiceDetail"]["radio"]["id"], "29");
    assert!(matches!(
        started.track.unwrap().voice_id.as_deref(),
        Some("1" | "2" | "3")
    ));
    assert_eq!(
        client
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["5", "1", "2", "3"]
    );
}

#[tokio::test]
async fn classified_podcast_next_inserts_all_pages_after_the_paused_entry_without_loading() {
    let (dir, api, _session, client, device) = fixture_with_device();
    seed_search(&api);
    store_checkpoint(dir.path());
    api.count.store(251, Ordering::SeqCst);
    let row = row_receipt(&client).await;
    let (card, _) = receipts(&client).await;
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    let start = prepared(&client, PlaySelection::Voice { id: "1".into() })
        .await
        .unwrap();
    client
        .player_action(
            "1",
            PlayerAction::QueueVoice {
                id: "400".into(),
                program: None,
                expected_queue_revision: start.queue_revision,
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
    let previous = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let prior_queue = client.player_queue("1").await.unwrap();
    replace_preference(&client, true);
    api.calls.lock().unwrap().clear();
    let next = SearchPodcastRowGesture::Next;
    assert!(matches!(
        prepared(&client, select_row(&card, next)).await,
        Err(Error::QueueChanged)
    ));
    let list_calls = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/v6/dj/program/byradio")
            .map(|(_, args)| args.clone())
            .collect::<Vec<_>>()
    };
    assert!(list_calls().is_empty());

    let changed = prepared(&client, select_row(&row, next)).await.unwrap();
    assert_ne!(changed.queue_revision, previous.queue_revision);
    assert_eq!(changed.state, PlaybackStatus::Paused);
    assert_eq!(changed.play_id, previous.play_id);
    assert_eq!(changed.entry_id, previous.entry_id);
    assert_eq!(changed.position, previous.position);
    assert_eq!(changed.mode, previous.mode);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.current, prior_queue.current);
    assert_eq!(queue.tracks[0].entry_id, prior_queue.tracks[0].entry_id);
    assert_eq!(queue.tracks[251].entry_id, prior_queue.tracks[1].entry_id);
    let expected: Vec<_> = std::iter::once(1)
        .chain((2..=251).rev())
        .chain(std::iter::once(400))
        .map(|id| id.to_string())
        .collect();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|entry| entry.track.voice_id.as_ref().unwrap())
            .collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        list_calls(),
        [0, 250].map(|offset| json!({"radioId":"7","offset":offset,"limit":250,"asc":false}))
    );
    // A background read for a neighbouring entry does not reload the current one.
    assert!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .all(|(path, args)| match path.as_str() {
                "/api/v6/dj/program/byradio" => true,
                "/api/dj/program/detail" => matches!(args["id"].as_str(), Some("251" | "400")),
                "/api/dj/play/url/check" => matches!(args["voiceId"].as_str(), Some("251" | "400")),
                "/api/song/enhance/player/url/v1" =>
                    matches!(args["ids"].as_str(), Some("[\"1251\"]" | "[\"1400\"]")),
                _ => false,
            })
    );

    let stepped = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(stepped.track.unwrap().voice_id.as_deref(), Some("251"));
    let stepped = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    api.search_playback_list_hold.store(true, Ordering::SeqCst);
    let client = Arc::new(client);
    let pending = tokio::spawn({
        let client = client.clone();
        async move { prepared(&client, select_row(&row, next)).await }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        api.search_playback_started.notified(),
    )
    .await
    .unwrap();
    let newest = client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    api.search_playback_release.notify_one();
    assert!(pending.await.unwrap().is_err());
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.mode, PlaybackMode::Sequential);
    assert_eq!(after.play_id, stepped.play_id);
    assert_eq!(after.queue_revision, newest.queue_revision);
    assert_eq!(list_calls().len(), 3, "no stale second page");
}

#[tokio::test]
async fn classified_podcast_next_starts_an_empty_queue_at_the_checkpoint_without_forcing_order() {
    let (dir, api, _session, client, device) = fixture_with_device();
    store_checkpoint(dir.path());
    api.count.store(2, Ordering::SeqCst);
    let row = row_receipt(&client).await;
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
    let started = prepared(&client, select_row(&row, SearchPodcastRowGesture::Next))
        .await
        .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Random);
    assert_eq!(started.position, Some(19.0));
    assert_eq!(started.queue_len, 2);
    assert_eq!(started.track.unwrap().voice_id.as_deref(), Some("1"));
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let calls = api.calls.lock().unwrap();
    let expected = [
        (
            "/api/v6/dj/program/byradio",
            json!({"radioId":"7","offset":0,"limit":250,"asc":false}),
        ),
        ("/api/dj/program/detail", json!("1")),
        ("/api/dj/play/url/check", json!("1")),
        ("/api/song/enhance/player/url/v1", json!("[\"1001\"]")),
        // The controller can prefetch the other episode as soon as playback starts.
        ("/api/dj/program/detail", json!("2")),
        ("/api/dj/play/url/check", json!("2")),
        ("/api/song/enhance/player/url/v1", json!("[\"1002\"]")),
    ];
    assert!((4..=expected.len()).contains(&calls.len()));
    for ((path, args), (expected_path, expected_args)) in calls.iter().zip(expected) {
        assert_eq!(path, expected_path);
        let actual = match path.as_str() {
            "/api/dj/program/detail" => &args["id"],
            "/api/dj/play/url/check" => &args["voiceId"],
            "/api/song/enhance/player/url/v1" => &args["ids"],
            _ => args,
        };
        assert_eq!(actual, &expected_args);
    }
}
