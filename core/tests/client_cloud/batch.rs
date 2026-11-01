use super::*;
#[tokio::test]
async fn selected_cloud_playback_keeps_native_order_and_appends_without_restarting_current_audio() {
    let f = Fixture::new(3);
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    let started = client
        .play_selection(
            "1",
            PlaySelection::CloudBatch {
                query: None,
                ids: vec!["3".into(), "1".into()],
                revision: revision.clone(),
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let before = client.player_queue("1").await.unwrap();
    assert_eq!(
        before
            .tracks
            .iter()
            .map(|v| v.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "3"]
    );
    let added = client
        .player_action(
            "1",
            PlayerAction::QueueCloudBatch {
                query: None,
                ids: vec!["2".into()],
                revision: revision.clone(),
                quality: Quality::Standard,
                expected_queue_revision: before.revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(added.play_id, started.play_id);
    let after = client.player_queue("1").await.unwrap();
    assert_eq!(
        after
            .tracks
            .iter()
            .map(|v| v.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert!(
        client
            .player_action(
                "1",
                PlayerAction::QueueCloudBatch {
                    query: None,
                    ids: vec!["2".into()],
                    revision: revision.clone(),
                    quality: Quality::Standard,
                    expected_queue_revision: before.revision
                }
            )
            .await
            .is_err()
    );
    for ids in [vec!["1".into(), "1".into()], vec!["99999".into()]] {
        assert!(
            client
                .play_selection(
                    "1",
                    PlaySelection::CloudBatch {
                        query: None,
                        ids,
                        revision: revision.clone(),
                        quality: Quality::Standard
                    }
                )
                .await
                .is_err()
        );
    }
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p == "/api/v3/song/detail")
    );
    assert!(!serde_json::to_string(&after).unwrap().contains("SECRET"));
}
#[tokio::test]
async fn a_changed_catalog_or_account_cannot_play_a_captured_cloud_selection() {
    let f = Fixture::new(3);
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    f.api.rows.lock().unwrap().push(row(4));
    assert!(
        client
            .play_selection(
                "1",
                PlaySelection::CloudBatch {
                    query: None,
                    ids: vec!["1".into()],
                    revision: revision.clone(),
                    quality: Quality::Standard
                }
            )
            .await
            .is_err()
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(
        client
            .play_selection(
                "1",
                PlaySelection::CloudBatch {
                    query: None,
                    ids: vec!["1".into()],
                    revision,
                    quality: Quality::Standard
                }
            )
            .await
            .is_err()
    );
}
