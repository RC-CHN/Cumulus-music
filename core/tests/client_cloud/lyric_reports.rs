use super::*;
#[tokio::test]
async fn lyric_report_uses_private_cloud_id_and_cloud_version_instead_of_metadata_song_id() {
    let f = Fixture::new(3);
    let client = f.client();
    let query = query(CloudTab::Uploaded);
    let page = client.cloud_page("1", query.clone(), true).await.unwrap();
    let selected = client
        .play_selection(
            "1",
            PlaySelection::Cloud {
                query,
                start_id: Some("2".into()),
                revision: page.revision.unwrap(),
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let opened = client
        .lyric_report_open("1", selected.play_id.as_deref().unwrap())
        .await
        .unwrap();
    let loaded = client
        .lyric_report_types("1", &opened.id, &opened.revision)
        .await
        .unwrap();
    let result = client
        .lyric_report_submit(
            "1",
            &loaded.id,
            &loaded.revision,
            "10",
            "云盘歌词内容需要核对",
        )
        .await
        .unwrap();
    assert_eq!(result.phase, cumulus_core::lyrics::reports::Phase::Sent);
    let calls = f.api.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|(p, v)| p == "/api/cloud/lyric/get" && v["songId"] == "2")
    );
    assert!(!calls.iter().any(|(p, _)| p == "/api/song/lyric/v1"));
    let requests = calls
        .iter()
        .filter(|(p, _)| p == "/api/feedback/correct/song")
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].1,
        json!({"songId":"2","lyricVersion":-1,"correctType":10,"content":"云盘歌词内容需要核对"})
    );
    assert!(!serde_json::to_string(&result).unwrap().contains("SECRET"));
}
