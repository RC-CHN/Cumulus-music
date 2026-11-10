use super::*;
use cumulus_core::playback::controller::PlaybackStatus;
use std::time::Duration;

pub(super) fn response(payload: &Value) -> Value {
    let ids = if payload["offset"] == 0 {
        [4, 5, 6]
    } else {
        [2, 7, 8]
    };
    let resources: Vec<_> = ids
        .into_iter()
        .map(|id| {
            json!({"baseInfo":{"simpleSongData":{"id":id,
                "name":format!("合成搜索歌曲 {id}"),"dt":120000,
                "ar":[{"id":101,"name":"合成歌手"}],"al":{"id":123,"name":"合成专辑"},
                "privilege":{"cs":id==6,"st":if id==5 {-200}else{0},"pl":if id==5 {0}else{320000}}}}})
        })
        .collect();
    json!({"code":200,"data":{"resources":resources,"totalCount":6,"hasMore":payload["offset"]==0}})
}

fn query(offset: usize) -> SearchResultQuery {
    SearchResultQuery {
        keyword: "合成".into(),
        category: SearchCategory::Songs,
        offset,
        limit: 3,
        correct: true,
        channel: None,
    }
}

fn drop(view: &PlayerView, before: Option<String>, rows: Vec<Value>) -> PlayerAction {
    serde_json::from_value(json!({"kind":"drop_search_songs_into_queue","rows":rows,
        "quality":"standard","before_entry_id":before,"expected_play_id":view.play_id,
        "expected_queue_revision":view.queue_revision}))
    .unwrap()
}

fn search_row(section: &Value, index: usize) -> Value {
    let row = &section["sections"][0]["items"][index];
    json!({"receipt":row["queueDropReceipt"],"track_id":row["track"]["id"]})
}

#[tokio::test]
async fn search_drag_inserts_only_native_observed_rows_from_both_pages_without_restarting() {
    let (_dir, api, session, client, _) = fixture();
    // Song 2 is the original queue's ordinary next-track prefetch. Confirm
    // its request was already recorded, and hold it while checking the drop
    // so neither the old nor the new neighbor races the wire journal.
    api.hold_url_id.store(2, Ordering::SeqCst);
    let _ = client
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
    let playing = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), api.url_started.notified())
        .await
        .expect("the original next track reached URL prefetch");
    let before = client.player_queue("1").await.unwrap();
    let p1 =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    let p2 =
        serde_json::to_value(client.search_results("1", query(3)).await.unwrap().value).unwrap();
    assert!(search_row(&p1, 0)["receipt"].is_string());
    assert!(search_row(&p1, 2)["receipt"].is_null());
    assert_ne!(search_row(&p1, 0)["receipt"], search_row(&p2, 0)["receipt"]);
    let next = before.tracks[2].entry_id.clone();
    let rows = vec![search_row(&p2, 1), search_row(&p1, 0), search_row(&p2, 0)];
    let mut forged = rows[0].clone();
    forged["track_id"] = json!("4");
    api.calls.lock().unwrap().clear();
    assert!(
        client
            .player_action("1", drop(&playing, Some(next.clone()), vec![forged]))
            .await
            .is_err()
    );
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        before.revision
    );
    let result = client
        .player_action("1", drop(&playing, Some(next.clone()), rows.clone()))
        .await
        .unwrap();
    assert_eq!(result.play_id, playing.play_id);
    assert_eq!(result.state, PlaybackStatus::Paused);
    assert_eq!(result.position, playing.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|r| r.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "7", "4", "2", "3"]
    );
    assert_eq!(queue.current, before.current);
    assert_eq!(queue.tracks[3].entry_id, before.tracks[1].entry_id);
    assert!(queue.tracks[1].track.name.contains("合成搜索"));
    assert!(matches!(
        client
            .player_action("1", drop(&playing, Some(next), rows.clone()))
            .await,
        Err(Error::QueueChanged)
    ));
    {
        let calls = api.calls.lock().unwrap();
        assert!(calls.is_empty(), "search drop issued requests: {calls:?}");
    }
    api.hold_url_id.store(0, Ordering::SeqCst);
    api.url_release.notify_one();

    let token = session.token().unwrap();
    session
        .adopt(api.clone(), json!({"userId":11}), &token)
        .unwrap();
    let other = client.play_selection("2", songs("1")).await.unwrap();
    assert!(matches!(
        client.player_action("2", drop(&other, None, rows)).await,
        Err(Error::State("search drop results expired"))
    ));
    assert_eq!(client.player_queue("2").await.unwrap().tracks.len(), 1);
}

fn group_next(view: &PlayerView, anchor: Value, rows: Vec<Value>) -> PlayerAction {
    serde_json::from_value(json!({
        "kind":"search_group_next_play", "anchor":anchor, "rows":rows, "quality":"standard",
        "expected_play_id":view.play_id, "expected_queue_revision":view.queue_revision
    }))
    .unwrap()
}

#[tokio::test]
async fn search_group_next_queues_in_selection_order_filters_and_preserves_the_decoder() {
    let (dir, api, session, client, _) = fixture();
    api.search_menu_revision.store(1, Ordering::SeqCst);
    api.hold_url_id.store(2, Ordering::SeqCst);
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
    let playing = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), api.url_started.notified())
        .await
        .expect("the existing next-source prefetch started");
    let before = client.player_queue("1").await.unwrap();
    let result = |view| serde_json::to_value(view).unwrap();
    let p1 = result(client.search_results("1", query(0)).await.unwrap().value);
    let p2 = result(client.search_results("1", query(3)).await.unwrap().value);
    let mut other_query = query(3);
    other_query.keyword = "另一个搜索".into();
    let unrelated = result(client.search_results("1", other_query).await.unwrap().value);
    assert_eq!(p1["sections"][0]["items"][0]["groupNextRight"], "playable");
    assert_eq!(
        p1["sections"][0]["items"][1]["groupNextRight"],
        "copyright_blocked"
    );
    assert!(p1["sections"][0]["items"][2]["groupNextRight"].is_null());
    let anchor = search_row(&p1, 0);
    let blocked = search_row(&p1, 1);
    let rows = vec![
        search_row(&p2, 1),
        blocked.clone(),
        search_row(&p2, 0),
        anchor.clone(),
    ];
    let mut forged = rows[0].clone();
    forged["track_id"] = json!("99");
    api.calls.lock().unwrap().clear();
    for (anchor, rows) in [
        (blocked, rows.clone()),
        (
            anchor.clone(),
            vec![anchor.clone(), search_row(&unrelated, 0)],
        ),
        (anchor.clone(), vec![anchor.clone(), forged]),
        (anchor.clone(), vec![anchor.clone(), anchor.clone()]),
        (anchor.clone(), vec![search_row(&p2, 0), search_row(&p2, 1)]),
    ] {
        assert!(
            client
                .player_action("1", group_next(&playing, anchor, rows))
                .await
                .is_err()
        );
        assert_eq!(
            client.player_queue("1").await.unwrap().revision,
            before.revision
        );
    }
    let updated = client
        .player_action("1", group_next(&playing, anchor.clone(), rows.clone()))
        .await
        .unwrap();
    assert_eq!(updated.play_id, playing.play_id);
    assert_eq!(updated.state, PlaybackStatus::Paused);
    assert_eq!(updated.position, playing.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.current, before.current);
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|item| item.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "7", "2", "4", "3"]
    );
    assert_eq!(queue.tracks[2].entry_id, before.tracks[1].entry_id);
    let saved: cumulus_core::playback::QueueSnapshot = serde_json::from_value(
        cumulus_core::storage::StateStore::open(dir.path())
            .unwrap()
            .get("9", "playback", "queue")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    for entry in &saved.items[1..4] {
        assert_eq!(entry.source["scene"], "search");
        assert_eq!(entry.source["trial_mode"], 42);
        assert_eq!(entry.resource["trial_mode"], 42);
    }
    assert_ne!(saved.items[0].source["scene"], "search");

    assert!(matches!(
        client
            .player_action("1", group_next(&playing, anchor.clone(), rows.clone()))
            .await,
        Err(Error::QueueChanged)
    ));
    api.search_menu_revision.store(2, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", group_next(&updated, anchor.clone(), rows.clone()))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        queue.revision
    );
    assert!(
        api.calls.lock().unwrap().is_empty(),
        "no new search/detail/source request"
    );
    api.hold_url_id.store(0, Ordering::SeqCst);
    api.url_release.notify_one();

    let token = session.token().unwrap();
    session
        .adopt(api.clone(), json!({"userId":11}), &token)
        .unwrap();
    assert!(matches!(
        client
            .player_action("2", group_next(&updated, anchor, rows))
            .await,
        Err(Error::StaleResponse)
    ));
}
