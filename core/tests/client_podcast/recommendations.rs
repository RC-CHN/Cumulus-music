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
    let rows = projected["value"].as_array().unwrap();
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
            .value[0]
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
