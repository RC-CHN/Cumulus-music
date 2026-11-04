mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{ScriptApi, gate};

fn client(api: Arc<ScriptApi>, path: &std::path::Path) -> (Arc<MusicClient>, Arc<AccountSession>) {
    let registry = ProfileRegistry::open(path, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    (
        Arc::new(MusicClient::with_session(session.clone(), registry)),
        session,
    )
}

#[tokio::test]
async fn exact_default_read_projects_ordered_tracks_and_applied_tags() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([json!({"code":200,"data":{
        "dailySongs":[
            {"id":72,"name":"Song B","dt":2000,"ar":[{"name":"Artist"}],"al":{"name":"Album"},"url":"https://example.invalid/private"},
            {"id":"71","name":"Song A"}
        ],
        "tags":{"categoryId":12,"categoryName":" Applied category ","private":"sentinel","tagVOList":[
            {"tagId":7,"tagName":"First style","private":"sentinel"},
            {"tagId":"8","tagName":"Second style","url":"https://example.invalid/private"}
        ]}
    }}).into()]));
    let (client, _) = client(api.clone(), root.path());
    let data = client.style_recommendations("1").await.unwrap();
    assert_eq!(data.epoch, "1");
    assert_eq!(data.value.total, 2);
    assert_eq!(data.value.offset, 0);
    assert!(!data.value.more && data.value.revision.is_some() && data.value.playlist.is_none());
    let tracks = &data.value.tracks;
    assert_eq!(
        tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["72", "71"]
    );
    assert_eq!(tracks[0].artists, ["Artist"]);
    assert_eq!(tracks[0].album, "Album");
    assert_eq!(tracks[0].duration_ms, 2000);
    let view = serde_json::to_value(&data).unwrap();
    assert_eq!(
        view["value"]["styleTags"],
        json!({
            "id":"12","name":"Applied category","tags":[
                {"id":"7","name":"First style"},{"id":"8","name":"Second style"}
            ]
        })
    );
    let serialized = view.to_string();
    assert!(!serialized.contains("sentinel") && !serialized.contains("example.invalid"));
    let calls = api.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "/api/homepage/category/daily/song/list");
    assert_eq!(
        calls[0].payload,
        json!({"source":"","categoryId":"","tagId":"","songId":""})
    );
    assert!(!calls[0].cache);
}

#[tokio::test]
async fn absent_or_invalid_style_tags_do_not_discard_tracks_or_trigger_another_read() {
    let root = tempfile::tempdir().unwrap();
    let responses = [
        Value::Null,
        json!({}),
        json!({"categoryId":1,"categoryName":"Style","tagVOList":[]}),
        json!({"categoryId":1,"categoryName":"Style","tagVOList":"invalid"}),
        json!({"categoryId":1,"categoryName":"Style","tagVOList":[
            {"tagId":2,"tagName":"Repeated"},{"tagId":"2","tagName":"Repeated"}
        ]}),
        json!({"categoryId":1,"categoryName":"Style","tagVOList":vec![Value::Null;21]}),
    ];
    let reads = responses.len();
    let api = Arc::new(ScriptApi::new(responses.map(|tags| {
        json!({"code":200,"data":{"dailySongs":[{"id":71}],"tags":tags}}).into()
    })));
    let (client, _) = client(api.clone(), root.path());
    for _ in 0..reads {
        let data = client.style_recommendations("1").await.unwrap();
        assert_eq!(data.value.tracks[0].id, "71");
        assert!(data.value.style_tags.is_none());
        assert!(
            serde_json::to_value(data).unwrap()["value"]
                .get("styleTags")
                .is_none()
        );
    }
    let calls = api.calls();
    assert_eq!(calls.len(), reads);
    assert!(
        calls
            .iter()
            .all(|call| call.path == "/api/homepage/category/daily/song/list")
    );
}

#[tokio::test]
async fn malformed_and_duplicate_tracks_fail_without_partial_results() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"tags":{}}}).into(),
        json!({"code":200,"data":{"dailySongs":"invalid"}}).into(),
        json!({"code":200,"data":{"dailySongs":[{"id":1},{"id":"1"}]}}).into(),
        json!({"code":200,"data":{"dailySongs":vec![Value::Null;501]}}).into(),
    ]));
    let (client, _) = client(api, root.path());
    assert!(
        client
            .style_recommendations("1")
            .await
            .unwrap()
            .value
            .tracks
            .is_empty()
    );
    for _ in 0..3 {
        assert!(matches!(
            client.style_recommendations("1").await,
            Err(Error::Response(_))
        ));
    }
}

#[tokio::test]
async fn prior_epoch_is_rejected_and_pending_result_does_not_cross_logout() {
    let root = tempfile::tempdir().unwrap();
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([pending]));
    let (client, session) = client(api.clone(), root.path());
    assert!(matches!(
        client.style_recommendations("0").await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls().is_empty());
    let reading = tokio::spawn(async move { client.style_recommendations("1").await });
    api.wait_for_calls(1).await;
    session.detach().unwrap();
    send.send(Ok(
        json!({"code":200,"data":{"dailySongs":[{"id":71,"name":"Old account"}],"tags":{
            "categoryId":"old","categoryName":"Old category","tagVOList":[{"tagId":"old","tagName":"Old account style"}]
        }}}),
    ))
    .unwrap();
    assert!(matches!(reading.await.unwrap(), Err(Error::StaleResponse)));
}

#[tokio::test]
async fn restoring_a_visible_version_fences_an_inflight_refresh_without_fetching() {
    let root = tempfile::tempdir().unwrap();
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"dailySongs":[{"id":61}]}}).into(),
        pending,
    ]));
    let (client, _) = client(api.clone(), root.path());
    let original = client.style_recommendations("1").await.unwrap().value;
    let refresh = tokio::spawn({
        let client = client.clone();
        async move { client.style_recommendations("1").await }
    });
    api.wait_for_calls(2).await;
    let restored = client
        .restore_style_recommendations("1", original.revision.as_deref().unwrap(), None)
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(restored.revision, original.revision);
    assert_eq!(restored.tracks[0].id, "61");
    release
        .send(Ok(json!({"code":200,"data":{"dailySongs":[{"id":62}]}})))
        .unwrap();
    assert!(matches!(refresh.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 2);
}
