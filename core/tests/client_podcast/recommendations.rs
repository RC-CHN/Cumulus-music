use super::*;

#[tokio::test]
async fn subscription_recommendations_use_exact_uncached_read_and_strip_raw_payload() {
    let (_dir, api, _session, client) = fixture();
    *api.recommendation_response.lock().unwrap() = json!({
        "code": 200,
        "data": [
            {"resourceId":"9007199254740993","voiceListName":"First",
             "coverUrl":"https://p1.music.126.net/art.jpg?token=SECRET",
             "playCount":"187","cookie":"SECRET"},
            {"id":77,"name":"Second","underShelf":true,"playCount":9_007_199_254_740_992_u64},
            {"id":77,"name":"duplicate"},
            {"id":"foreign","name":"invalid"},
            {"id":32,"name":"Third","playCount":0},
            {"id":59,"name":"Fourth","coverUrl":"https://invalid.example/?token=SECRET"},
            {"id":50,"name":"Beyond carousel"}
        ]
    });
    let initial = client
        .podcast_subscription_recommendations("1")
        .await
        .unwrap();
    let projected = serde_json::to_value(initial).unwrap();
    let rows = projected["value"]["cards"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["9007199254740993", "77", "32", "59"]
    );
    assert_eq!(projected["epoch"], "1");
    assert_eq!(rows[0]["playCount"], 187);
    assert_eq!(rows[1]["unavailable"], true);
    assert!(rows[1]["playCount"].is_null());
    assert!(rows[3]["coverUrl"].is_null());
    assert!(!projected.to_string().contains("SECRET"));

    *api.recommendation_response.lock().unwrap() = json!({"code":200,"data":[]});
    assert!(
        client
            .podcast_subscription_recommendations("1")
            .await
            .unwrap()
            .value
            .cards
            .is_empty()
    );
    *api.recommendation_response.lock().unwrap() = json!({"code":200,"data":{"id":2}});
    assert!(matches!(
        client.podcast_subscription_recommendations("1").await,
        Err(Error::Response(_))
    ));
    assert_eq!(
        api.calls.lock().unwrap().as_slice(),
        vec![("/api/djradio/my/radio/recommend".into(), json!({})); 3]
    );
}

#[tokio::test]
async fn subscription_recommendations_never_return_late_or_guest_reads() {
    let (_dir, old, session, client) = fixture();
    *old.recommendation_response.lock().unwrap() =
        json!({"code":200,"data":[{"id":41,"name":"Old"}]});
    old.recommendation_hold.store(true, Ordering::SeqCst);
    let client = Arc::new(client);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { client.podcast_subscription_recommendations("1").await })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        old.recommendation_started.notified(),
    )
    .await
    .unwrap();
    let new = Arc::new(Backend::default());
    *new.recommendation_response.lock().unwrap() =
        json!({"code":200,"data":[{"id":42,"name":"New"}]});
    session
        .adopt(new.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    old.recommendation_release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(
        client.podcast_subscription_recommendations("1").await,
        Err(Error::StaleResponse)
    ));
    assert!(new.calls.lock().unwrap().is_empty());
    assert_eq!(
        client
            .podcast_subscription_recommendations("2")
            .await
            .unwrap()
            .value
            .cards[0]
            .id,
        "42"
    );

    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let guest = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(guest.clone(), Some(registry.store())));
    let guest_client = MusicClient::with_session(session, registry);
    assert!(
        guest_client
            .podcast_subscription_recommendations("0")
            .await
            .is_err()
    );
    assert!(guest.calls.lock().unwrap().is_empty());
}

async fn prepared(client: &MusicClient, input: PlaySelection) -> cumulus_core::Result<PlayerView> {
    let current = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &current.queue_revision, current.play_id.as_deref())?;
    client.commit_prepared_selection("1", &ticket, input).await
}

#[tokio::test]
async fn subscription_recommendations_overlay_paginates_and_toggles_current_decoder() {
    use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

    let (dir, api, _session, client, device) = fixture_with_device();
    *api.recommendation_response.lock().unwrap() =
        json!({"code":200,"data":[radio(),{"id":8,"name":"Unavailable","underShelf":true}]});
    api.count.store(251, Ordering::SeqCst);
    let library = client
        .podcast_page("1", query(PodcastScope::Podcasts, 0), true)
        .await
        .unwrap();
    let shelf = client
        .podcast_subscription_recommendations("1")
        .await
        .unwrap()
        .value;
    let profile = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    profile
        .store()
        .put(
            "9",
            "podcast-latest-voice-v1",
            "7",
            &json!({
                "id":"251","mainTrackId":"1251","name":"Episode 251","time":19,"duration":120
            }),
        )
        .unwrap();
    let _subscription = client.subscribe_playback("1").await.unwrap();
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let select = |id: &str, receipt: &str| PlaySelection::PodcastSubscriptionRecommendation {
        id: id.into(),
        revision: library.revision.clone(),
        receipt: receipt.into(),
    };
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        prepared(&client, select("8", &shelf.receipt)).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        prepared(&client, select("7", "00000000000000000000000000000000")).await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    assert_eq!(device.state.lock().unwrap().loads, 0);

    let started = prepared(&client, select("7", &shelf.receipt))
        .await
        .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Sequential);
    assert_eq!(
        started.track.as_ref().unwrap().voice_id.as_deref(),
        Some("251")
    );
    assert_eq!(started.position, Some(19.0));
    assert!(matches!(started.origin, Some(PlayerOrigin::Podcast { ref id }) if id == "7"));
    assert_eq!(started.queue_len, 251);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let observed = api.calls.lock().unwrap().clone();
    let by_radio = "/api/v6/dj/program/byradio";
    assert_eq!(
        observed
            .iter()
            .filter(|(path, _)| path == by_radio)
            .map(|(_, args)| args.clone())
            .collect::<Vec<_>>(),
        [
            json!({"radioId":"7","offset":0,"limit":250}),
            json!({"radioId":"7","offset":250,"limit":250})
        ]
    );
    assert!(
        observed
            .iter()
            .any(|(path, p)| path == "/api/dj/play/url/check" && p["voiceId"] == "251")
    );
    assert!(!observed.iter().any(|(path, _)| matches!(
        path.as_str(),
        "/api/djradio/v3/get"
            | "/api/dj/around/program/v1"
            | "/api/dj/user/setting/resumeplay/get"
            | "/api/djradio/newcount/clear/byradio"
    )));

    for state in [PlaybackStatus::Paused, PlaybackStatus::Playing] {
        let toggled = prepared(&client, select("7", &shelf.receipt))
            .await
            .unwrap();
        assert_eq!(toggled.state, state);
        assert_eq!(toggled.play_id, started.play_id);
        assert_eq!(toggled.entry_id, started.entry_id);
        assert_eq!(toggled.position, started.position);
        assert_eq!(toggled.queue_revision, started.queue_revision);
    }
    assert_eq!(api.calls.lock().unwrap().as_slice(), observed.as_slice());
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn subscription_recommendations_overlay_uses_saved_order_and_treatment_resume_off() {
    use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

    let (dir, api, _session, client) = fixture();
    api.recommendation_resume_treatment
        .store(true, Ordering::SeqCst);
    client.refresh_pc_presentation().await.unwrap();
    api.count.store(2, Ordering::SeqCst);
    let mut card = radio();
    // A music category uses the existing mode rather than forcing sequential.
    card["categoryId"] = json!(3001);
    *api.recommendation_response.lock().unwrap() = json!({"code":200,"data":[card]});
    let library = client
        .podcast_page("1", query(PodcastScope::Podcasts, 0), true)
        .await
        .unwrap();
    let shelf = client
        .podcast_subscription_recommendations("1")
        .await
        .unwrap()
        .value;
    let profile = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    profile
        .store()
        .put("9", "podcast-radio-order-v1", "7", &json!(false))
        .unwrap();
    profile
        .store()
        .put(
            "9",
            "podcast-latest-voice-v1",
            "7",
            &json!({
                "id":"1","mainTrackId":"1001","name":"Episode 1","time":19,"duration":120
            }),
        )
        .unwrap();
    let _subscription = client.subscribe_playback("1").await.unwrap();
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    let result = prepared(
        &client,
        PlaySelection::PodcastSubscriptionRecommendation {
            id: "7".into(),
            revision: library.revision,
            receipt: shelf.receipt,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.state, PlaybackStatus::Playing);
    assert_eq!(result.track.unwrap().voice_id.as_deref(), Some("2"));
    assert_eq!(result.position, Some(0.0));
    assert_eq!(result.mode, PlaybackMode::SingleLoop);
    let requests = api.calls.lock().unwrap();
    assert_eq!(
        requests[0],
        (
            "/api/v6/dj/program/byradio".into(),
            json!({"radioId":"7","offset":0,"limit":250,"asc":false})
        )
    );
    assert_eq!(
        requests[1],
        (
            "/api/dj/user/setting/resumeplay/get".into(),
            json!({"radioId":"7"})
        )
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path == "/api/dj/user/setting/resumeplay/get")
            .count(),
        1
    );
}
