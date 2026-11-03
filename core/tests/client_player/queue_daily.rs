use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn add(revision: String, queue_revision: &str) -> PlayerAction {
    PlayerAction::QueueDailyRecommendations {
        revision,
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
