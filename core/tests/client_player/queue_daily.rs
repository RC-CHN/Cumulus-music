use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn add(revision: String, queue_revision: &str) -> PlayerAction {
    PlayerAction::QueueDailyRecommendations {
        revision,
        indices: None,
        quality: Quality::Standard,
        expected_queue_revision: queue_revision.into(),
    }
}

async fn daily_revision(client: &MusicClient) -> String {
    client.daily_songs().await.unwrap().value.revision.unwrap()
}

#[tokio::test]
async fn daily_append_keeps_paused_entry_and_binds_inserted_order_and_source() {
    let device = Arc::new(support::Device::default());
    let (dir, _api, _session, client, _) = fixture_with_device(device.clone());
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "73", "2"].into_iter().map(str::to_owned).collect(),
                index: 0,
                quality: Quality::Standard,
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
    let revision = daily_revision(&client).await;
    let after = client
        .player_action("1", add(revision.clone(), &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(
        (after.play_id, after.entry_id, after.position),
        (before.play_id, before.entry_id, before.position)
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    let items = stored["items"].as_array().unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item["resource"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["1", "74", "73", "2"]
    );
    for item in &items[1..3] {
        assert_eq!(item["resource"]["trial_mode"], 35);
        assert_eq!(
            item["source"],
            json!({"kind":"daily_recommendations","scene":"dailyRecommend",
            "href":"/dailyRecommend","resourceType":"dailyRecommend","revision":revision,"trial_mode":35})
        );
    }
    assert_ne!(items[0]["source"]["kind"], "daily_recommendations");
}

#[tokio::test]
async fn daily_append_starts_first_when_empty_and_rejects_stale_snapshot_or_queue() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _session, client, _) = fixture_with_device(device.clone());
    let stale = daily_revision(&client).await;
    let current = daily_revision(&client).await;
    let empty = client.player_snapshot("1").await.unwrap();
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .player_action("1", add(stale, &empty.queue_revision))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.player_action("1", add(current.clone(), "999")).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
    let added = client
        .player_action("1", add(current, &empty.queue_revision))
        .await
        .unwrap();
    assert_eq!(added.state, PlaybackStatus::Playing);
    assert_eq!(added.track.unwrap().id, "74");
    assert_eq!(added.queue_len, 2);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(35)));
}

#[tokio::test]
async fn refreshed_daily_snapshot_rejects_an_append_awaiting_track_details() {
    let (_dir, api, _session, client, _) = fixture();
    let snapshot = daily_revision(&client).await;
    let empty = client.player_snapshot("1").await.unwrap();
    api.hold_id.store(74, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .player_action("1", add(snapshot, &empty.queue_revision))
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), api.started.notified())
        .await
        .unwrap();
    daily_revision(&client).await;
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert!(client.player_queue("1").await.unwrap().tracks.is_empty());
}

#[tokio::test]
async fn selected_daily_append_validates_indices_and_keeps_official_order() {
    let (_dir, api, _session, client, _) = fixture();
    *api.daily_tracks.lock().unwrap() = Some(vec![
        json!({"id":74,"name":"第一首"}),
        json!({"id":73,"name":"未选择"}),
        json!({"id":72,"name":"第三首"}),
    ]);
    let revision = daily_revision(&client).await;
    let empty = client.player_snapshot("1").await.unwrap();
    let select = |indices| PlayerAction::QueueDailyRecommendations {
        revision: revision.clone(),
        indices: Some(indices),
        quality: Quality::Standard,
        expected_queue_revision: empty.queue_revision.clone(),
    };
    let before = api.calls.lock().unwrap().len();
    for invalid in [vec![], vec![0, 0], vec![3]] {
        assert!(matches!(
            client.player_action("1", select(invalid)).await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(api.calls.lock().unwrap().len(), before);
    let value = client.player_action("1", select(vec![2, 0])).await.unwrap();
    assert_eq!(value.track.unwrap().id, "74");
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["74", "72"]
    );
}

#[tokio::test]
async fn prepared_daily_batch_plays_the_first_selected_row_in_native_order() {
    let (_dir, api, _session, client, _) = fixture();
    *api.daily_tracks.lock().unwrap() = Some(vec![
        json!({"id":74,"name":"未选择"}),
        json!({"id":73,"name":"第一首选中"}),
        json!({"id":72,"name":"第二首选中"}),
    ]);
    let revision = daily_revision(&client).await;
    let initial = client.player_snapshot("1").await.unwrap();
    let select = |revision, indices| PlaySelection::DailyRecommendationsBatch {
        revision,
        indices,
        quality: Quality::Standard,
    };
    assert!(matches!(
        client
            .play_selection("1", select(revision.clone(), vec![]))
            .await,
        Err(Error::InvalidInput(_))
    ));
    let ticket = client
        .prepare_play_selection("1", &initial.queue_revision, initial.play_id.as_deref())
        .unwrap();
    let played = client
        .commit_prepared_selection("1", &ticket, select(revision, vec![2, 1]))
        .await
        .unwrap();
    assert_eq!(played.track.unwrap().id, "73");
    assert_eq!(played.state, PlaybackStatus::Playing);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(35)));
    assert_eq!(
        client
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["73", "72"]
    );
}
