use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};
use std::time::Duration;

pub(super) fn response(payload: &Value) -> Value {
    let ids = if payload["offset"] == 0 {
        &[11_u32, 12, 13][..]
    } else {
        &[14_u32, 15][..]
    };
    let resources: Vec<_> = ids
        .iter()
        .map(|&id| {
            json!({"song":{"id":id,"name":format!("合成歌词歌曲 {id}"),"dt":120000,
            "privilege":{"id":id,"st":if id==12 {-200}else{0},"pl":if id==12 {0}else{320000}}},
            "lyrics":format!("合成歌词 {id}")})
        })
        .collect();
    json!({"code":200,"data":{"resources":resources,"totalCount":5}})
}

fn query(offset: usize) -> SearchResultQuery {
    SearchResultQuery {
        keyword: "合成".into(),
        category: SearchCategory::Lyrics,
        offset,
        limit: 3,
        correct: true,
        channel: None,
    }
}

fn row(page: &Value, index: usize) -> &Value {
    &page["sections"][0]["items"][index]
}

fn receipt(page: &Value, index: usize) -> String {
    row(page, index)["lyricGroupPlayback"]["receipt"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn action(view: &PlayerView, operation: &str, anchor: &str, rows: &[String]) -> PlayerAction {
    serde_json::from_value(json!({
        "kind":"search_lyric_group_playback", "operation":operation,
        "anchor":anchor,"rows":rows,"quality":"standard",
        "expected_play_id":view.play_id,"expected_queue_revision":view.queue_revision
    }))
    .unwrap()
}

fn tracks(queue: &QueueView) -> Vec<&str> {
    queue
        .tracks
        .iter()
        .map(|row| row.track.id.as_str())
        .collect()
}

#[tokio::test]
async fn lyric_group_plays_the_clicked_song_then_queues_another_ordered_group_without_restart() {
    let device = Arc::new(support::Device::default());
    let (dir, api, _, client, _) = fixture_with_device(device.clone());
    api.search_menu_revision.store(1, Ordering::SeqCst);
    api.playlist_trial.enable_media_trace();
    let empty = client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let first =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    let second =
        serde_json::to_value(client.search_results("1", query(3)).await.unwrap().value).unwrap();
    let anchor = receipt(&first, 0);
    let blocked = receipt(&first, 1);
    let selection = vec![receipt(&second, 0), blocked.clone(), anchor.clone()];
    assert_eq!(
        row(&first, 1)["lyricGroupPlayback"]["right"],
        "copyright_blocked"
    );
    for field in ["queueDropReceipt", "groupDownload", "songShareReceipt"] {
        assert!(row(&first, 0).get(field).is_none());
    }

    assert!(matches!(
        client
            .player_action("1", action(&empty, "play", &blocked, &selection))
            .await,
        Err(Error::State(_))
    ));
    let wrong = serde_json::from_value::<PlayerAction>(json!({
        "kind":"drop_search_songs_into_queue","rows":[{"receipt":anchor,"track_id":"11"}],
        "quality":"standard","before_entry_id":null,"expected_play_id":empty.play_id,
        "expected_queue_revision":empty.queue_revision
    }))
    .unwrap();
    assert!(client.player_action("1", wrong).await.is_err());
    let wrong_group = serde_json::from_value::<PlayerAction>(json!({
        "kind":"search_group_play","rows":[{"receipt":anchor,"track_id":"11"},{"receipt":selection[0],"track_id":"14"}],
        "anchor":{"receipt":anchor,"track_id":"11"},"quality":"standard",
        "expected_play_id":empty.play_id,"expected_queue_revision":empty.queue_revision
    })).unwrap();
    assert!(client.player_action("1", wrong_group).await.is_err());
    assert!(api.playlist_trial.modes("11").is_empty());
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        empty.queue_revision
    );

    let active = client
        .player_action("1", action(&empty, "play", &anchor, &selection))
        .await
        .unwrap();
    assert_eq!(active.state, PlaybackStatus::Playing);
    assert_eq!(active.track.as_ref().unwrap().id, "11");
    assert_eq!(api.playlist_trial.modes("11"), [42]);
    let before = client.player_queue("1").await.unwrap();
    assert_eq!(tracks(&before), ["14", "11"]);
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let next_anchor = receipt(&second, 1);
    let next_rows = vec![receipt(&first, 2), next_anchor.clone()];
    let inserted = client
        .player_action("1", action(&paused, "next", &next_anchor, &next_rows))
        .await
        .unwrap();
    assert_eq!(inserted.state, PlaybackStatus::Paused);
    assert_eq!(inserted.play_id, paused.play_id);
    assert_eq!(inserted.position, paused.position);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let after = client.player_queue("1").await.unwrap();
    assert_eq!(after.current, before.current);
    assert_eq!(tracks(&after), ["14", "11", "13", "15"]);

    let store = cumulus_core::storage::StateStore::open(dir.path()).unwrap();
    let saved: cumulus_core::playback::QueueSnapshot =
        serde_json::from_value(store.get("9", "playback", "queue").unwrap().unwrap()).unwrap();
    assert!(
        saved
            .items
            .iter()
            .all(|entry| entry.source["scene"] == "search" && entry.resource["trial_mode"] == 42)
    );
    let current = saved
        .order
        .iter()
        .position(|id| Some(id) == saved.current.as_ref())
        .unwrap();
    assert_eq!(saved.order[current + 1], after.tracks[2].entry_id);
}

#[tokio::test]
async fn fresh_lyric_generation_does_not_commit_a_previous_generations_held_media_read() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _, client, _) = fixture_with_device(device.clone());
    api.search_menu_revision.store(1, Ordering::SeqCst);
    api.hold_url_id.store(11, Ordering::SeqCst);
    let empty = client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let first =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    let anchor = receipt(&first, 0);
    let rows = vec![receipt(&first, 2), anchor.clone()];
    let pending = client.player_action("1", action(&empty, "next", &anchor, &rows));
    tokio::pin!(pending);
    tokio::select! {
        reached = tokio::time::timeout(Duration::from_secs(5), api.url_started.notified()) => {
            reached.expect("the exact clicked lyric song reached the held media read");
        },
        response = &mut pending => panic!("media read was not held: {:?}", response.err()),
    }
    let fresh =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    assert_ne!(receipt(&fresh, 0), anchor);
    api.url_release.notify_one();
    assert!(pending.await.is_err());
    assert_eq!(device.state.lock().unwrap().loads, 0);
    let queue = client.player_queue("1").await.unwrap();
    assert!(queue.tracks.is_empty());
    assert_eq!(queue.revision, empty.queue_revision);
}
