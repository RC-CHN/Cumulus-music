use super::*;

#[tokio::test]
async fn cloud_filter_matches_the_recovered_origin_predicate_and_preserves_cloud_queue_order() {
    let f = Fixture::new(0);
    let vectors: Value =
        serde_json::from_str(include_str!("../fixtures/cloud-list-filter.json")).unwrap();
    *f.api.rows.lock().unwrap() = vectors["rows"].as_array().unwrap().clone();
    let client = f.client();
    let full = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert_eq!(full.total, 8);
    assert_eq!(full.tracks[1].track.name, "兜底文件.flac");
    let revision = full.revision.unwrap();
    let baseline = f.api.calls.lock().unwrap().clone();
    for vector in vectors["queries"].as_array().unwrap() {
        let page = client
            .cloud_page(
                "1",
                CloudQuery {
                    keyword: vector[0].as_str().unwrap().into(),
                    ..query(CloudTab::Uploaded)
                },
                false,
            )
            .await
            .unwrap();
        let ids: Vec<_> = page.tracks.iter().map(|row| &row.track.id).collect();
        assert_eq!(json!(ids), vector[1], "query {}", vector[0]);
        assert_eq!(page.revision.as_deref(), Some(revision.as_str()));
    }
    assert_eq!(*f.api.calls.lock().unwrap(), baseline);
    assert_eq!(
        baseline,
        [(support::LIST.to_owned(), json!({"offset":0,"limit":200}))]
    );

    // Selection uses the same complete filtered set, not a single visible
    // row or the public catalog IDs carried by those cloud songs.
    let selected = client
        .play_selection(
            "1",
            PlaySelection::Cloud {
                query: CloudQuery {
                    keyword: "樂隊 夜場".into(),
                    limit: 1,
                    ..query(CloudTab::Uploaded)
                },
                start_id: Some("1001".into()),
                revision,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.track.unwrap().id, "1001");
    let queue = client.player_queue("1").await.unwrap();
    let ids: Vec<_> = queue
        .tracks
        .iter()
        .map(|row| row.track.id.as_str())
        .collect();
    assert_eq!(ids, ["1002", "1001"]);
    assert!(f.api.calls.lock().unwrap().iter().all(|(path, _)| {
        matches!(
            path.as_str(),
            support::LIST | "/api/song/enhance/player/url/v1"
        )
    }));
}
