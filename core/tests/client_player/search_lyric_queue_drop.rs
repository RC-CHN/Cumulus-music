use super::*;

fn drop_receipt(page: &Value, index: usize) -> String {
    row(page, index)["lyricQueueDrop"]["receipt"]
        .as_str()
        .unwrap()
        .into()
}

fn drop_action(
    view: &PlayerView,
    before: Option<&str>,
    anchor: &str,
    rows: &[String],
) -> PlayerAction {
    serde_json::from_value(json!({
        "kind":"drop_search_lyrics_into_queue","anchor":anchor,"rows":rows,
        "quality":"standard","before_entry_id":before,"expected_play_id":view.play_id,
        "expected_queue_revision":view.queue_revision
    }))
    .unwrap()
}

#[tokio::test]
async fn lyric_drop_skips_blocked_grabbed_song_and_preserves_exact_paused_decoder() {
    let device = Arc::new(support::Device::default());
    let (dir, api, session, client, _) = fixture_with_device(device.clone());
    api.search_menu_revision.store(1, Ordering::SeqCst);
    api.playlist_trial.enable_media_trace();
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
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "11", "3"].map(str::to_owned).to_vec(),
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 22.0 })
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.position, Some(22.0));
    let before = client.player_queue("1").await.unwrap();
    let first =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    let later =
        serde_json::to_value(client.search_results("1", query(3)).await.unwrap().value).unwrap();
    let mut song_query = query(0);
    song_query.category = SearchCategory::Songs;
    let song_results =
        serde_json::to_value(client.search_results("1", song_query).await.unwrap().value).unwrap();
    let first_gap = Some(before.tracks[0].entry_id.as_str());
    let grabbed = drop_receipt(&first, 1);
    let playable = drop_receipt(&first, 0);
    let selected = [
        drop_receipt(&later, 0),
        grabbed.clone(),
        playable.clone(),
        drop_receipt(&first, 2),
    ];
    assert_eq!(
        row(&first, 1)["lyricQueueDrop"]["right"],
        "copyright_blocked"
    );
    assert_ne!(grabbed, receipt(&first, 1));
    assert!(row(&first, 1).get("queueDropReceipt").is_none());
    assert!(row(&song_results, 0).get("lyricQueueDrop").is_none());
    let song_grant = row(&song_results, 0)["queueDropReceipt"]
        .as_str()
        .unwrap()
        .to_owned();
    let menu_grant = receipt(&first, 0);
    api.calls.lock().unwrap().clear();
    for foreign in [song_grant, menu_grant] {
        assert!(matches!(
            client
                .player_action(
                    "1",
                    drop_action(&paused, first_gap, &foreign, std::slice::from_ref(&foreign))
                )
                .await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(matches!(
        client
            .player_action(
                "1",
                drop_action(&paused, first_gap, &grabbed, std::slice::from_ref(&grabbed))
            )
            .await,
        Err(Error::State("selected lyric songs are unavailable online"))
    ));
    assert!(matches!(
        client
            .player_action(
                "1",
                action(
                    &paused,
                    "play",
                    &playable,
                    &[playable.clone(), selected[0].clone()]
                )
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let ordinary: PlayerAction = serde_json::from_value(json!({
        "kind":"drop_search_songs_into_queue", "rows":[{"track_id":"11","receipt":playable}],
        "quality":"standard","before_entry_id":first_gap,"expected_play_id":paused.play_id,
        "expected_queue_revision":paused.queue_revision
    }))
    .unwrap();
    assert!(client.player_action("1", ordinary).await.is_err());
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        before.revision
    );

    // M308 interprets the first visual gap (-1) as after the current item.
    // Unlike menu Play, the grabbed blocked row is not a start requirement.
    let after = client
        .player_action("1", drop_action(&paused, first_gap, &grabbed, &selected))
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(tracks(&queue), ["1", "14", "11", "13", "3"]);
    assert_eq!(queue.current, before.current);
    assert_eq!(queue.tracks[2].entry_id, before.tracks[1].entry_id);
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, paused.play_id);
    assert_eq!(after.entry_id, paused.entry_id);
    assert_eq!(after.position, paused.position);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert!(
        api.calls.lock().unwrap().is_empty(),
        "ordinary drop issued a request"
    );
    let store = cumulus_core::storage::StateStore::open(dir.path()).unwrap();
    let saved: cumulus_core::playback::QueueSnapshot =
        serde_json::from_value(store.get("9", "playback", "queue").unwrap().unwrap()).unwrap();
    for item in [&saved.items[1], &saved.items[3]] {
        assert_eq!(item.source["scene"], "search");
        assert!(item.source.get("trial_mode").is_none());
        assert!(item.resource.get("trial_mode").is_none());
    }
    assert!(matches!(
        client
            .player_action("1", drop_action(&paused, first_gap, &grabbed, &selected))
            .await,
        Err(Error::QueueChanged)
    ));

    api.search_menu_revision.store(2, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", drop_action(&after, None, &grabbed, &selected))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        queue.revision
    );
    session
        .adopt(api, json!({"userId":11}), &session.token().unwrap())
        .unwrap();
    let other = client.play_selection("2", songs("1")).await.unwrap();
    assert!(matches!(
        client
            .player_action("2", drop_action(&other, None, &grabbed, &selected))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(tracks(&client.player_queue("2").await.unwrap()), ["1"]);
}
