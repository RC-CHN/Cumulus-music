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
async fn exact_default_read_projects_only_ordered_tracks() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([json!({"code":200,"data":{
        "dailySongs":[
            {"id":72,"name":"Song B","dt":2000,"ar":[{"name":"Artist"}],"al":{"name":"Album"},"url":"https://example.invalid/private"},
            {"id":"71","name":"Song A"}
        ],
        "tags":{"private":"sentinel"}
    }}).into()]));
    let (client, _) = client(api.clone(), root.path());
    let data = client.style_recommendations("1").await.unwrap();
    assert_eq!(data.epoch, "1");
    assert_eq!(data.value.total, 2);
    assert_eq!(data.value.offset, 0);
    assert!(!data.value.more && data.value.revision.is_none() && data.value.playlist.is_none());
    let tracks = &data.value.tracks;
    assert_eq!(
        tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["72", "71"]
    );
    assert_eq!(tracks[0].artists, ["Artist"]);
    assert_eq!(tracks[0].album, "Album");
    assert_eq!(tracks[0].duration_ms, 2000);
    let serialized = serde_json::to_string(&data).unwrap();
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
        json!({"code":200,"data":{"dailySongs":[{"id":71,"name":"Old account"}]}}),
    ))
    .unwrap();
    assert!(matches!(reading.await.unwrap(), Err(Error::StaleResponse)));
}
