mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, style_home::StyleSeedHandle},
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

fn handle(value: &Value, index: usize) -> StyleSeedHandle {
    serde_json::from_value(json!({"revision":value["revision"],"id":value["cards"][index]["id"]}))
        .unwrap()
}

fn card(title: &str, category: &str, tag: u64, target: &str, songs: Value) -> Value {
    json!({
        "resourceType":"dailySongs","subResourceType":"style_dailySong","moduleType":"tag_daily_rec",
        "title":title,"extData":{"categoryId":category,"tagId":tag},"targetUrl":target,"songIds":songs,
        "coverUrl":"http://p1.music.126.net/card.jpg?private=sentinel"
    })
}

#[tokio::test]
async fn projects_only_valid_cards_and_opens_native_seeds_with_exact_reads() {
    let root = tempfile::tempdir().unwrap();
    let good = card(
        "Seed card",
        "11",
        12,
        "https://music.163.com/style?songId=91%2C92&secret=sentinel",
        json!([99]),
    );
    let fallback = card("Fallback card", "21", 22, "", json!([93, "94"]));
    let invalid = card(
        "Invalid card",
        "31",
        32,
        "https://music.163.com/style?songId=private",
        json!([95]),
    );
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"items":[good.clone(),good,invalid,{"resourceType":"fm"}],
            "extInfo":{"pcRcmdDemoteItems":[fallback]}}}).into(),
        json!({"code":200,"data":{"dailySongs":[{"id":71},{"id":72}],"tags":{
            "categoryId":"11","categoryName":"Seed style","tagVOList":[{"tagId":12,"tagName":"Seed tag"}]
        }}}).into(),
        json!({"code":200,"data":{"dailySongs":[{"id":73}]}}).into(),
    ]));
    let (client, _) = client(api.clone(), root.path());
    let feed = serde_json::to_value(client.style_home_seeds("1").await.unwrap()).unwrap();
    assert_eq!(feed["epoch"], "1");
    let cards = feed["value"]["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0]["title"], "Seed card");
    assert_eq!(cards[1]["title"], "Fallback card");
    assert_eq!(
        cards[0]["coverUrl"],
        "https://p1.music.126.net/card.jpg?param=320y320"
    );
    let serialized = feed.to_string();
    for private in ["targetUrl", "songIds", "categoryId", "tagId", "sentinel"] {
        assert!(!serialized.contains(private), "{private}");
    }
    let list = client
        .seeded_style_recommendations("1", handle(&feed["value"], 0))
        .await
        .unwrap();
    assert_eq!(
        list.value
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["71", "72"]
    );
    assert_eq!(list.value.style_tags.as_ref().unwrap().name, "Seed style");
    let revision = list.value.revision.as_deref().unwrap();
    assert!(revision.starts_with("seeded-style-"));
    assert!(matches!(
        client
            .save_style_tag(
                "1",
                "61018dbb-89a9-43b0-b214-276b80c6abe9",
                revision,
                "11",
                "12"
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        client
            .seeded_style_recommendations("1", handle(&feed["value"], 1))
            .await
            .unwrap()
            .value
            .tracks[0]
            .id,
        "73"
    );
    let calls = api.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].path, "/api/pc/daily/rcmd/block");
    let client_time = calls[0].payload["clientTime"].as_str().unwrap();
    assert!(chrono::NaiveDateTime::parse_from_str(client_time, "%Y-%m-%d %H:%M:%S").is_ok());
    assert_eq!(calls[0].payload, json!({"clientTime":client_time}));
    for call in &calls[1..] {
        assert_eq!(call.path, "/api/homepage/category/daily/song/list");
        assert!(!call.cache);
    }
    assert_eq!(
        calls[1].payload,
        json!({"source":"homepage","categoryId":"11","tagId":"12","songId":"91,92"})
    );
    assert_eq!(
        calls[2].payload,
        json!({"source":"homepage","categoryId":"21","tagId":"22","songId":"93,94"})
    );
}

#[tokio::test]
async fn replacing_feed_or_account_rejects_old_seed_and_an_inflight_result() {
    let root = tempfile::tempdir().unwrap();
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"items":[card("First","1",2,"",json!([3]))]}}).into(),
        pending,
        json!({"code":200,"data":{"items":[]}}).into(),
    ]));
    let (client, session) = client(api.clone(), root.path());
    let feed = serde_json::to_value(client.style_home_seeds("1").await.unwrap()).unwrap();
    let seed = handle(&feed["value"], 0);
    let reading = tokio::spawn({
        let client = client.clone();
        async move { client.seeded_style_recommendations("1", seed).await }
    });
    api.wait_for_calls(2).await;
    client.style_home_seeds("1").await.unwrap();
    release
        .send(Ok(json!({"code":200,"data":{"dailySongs":[{"id":71}]}})))
        .unwrap();
    assert!(matches!(reading.await.unwrap(), Err(Error::QueueChanged)));
    assert!(matches!(
        client
            .seeded_style_recommendations("1", handle(&feed["value"], 0))
            .await,
        Err(Error::QueueChanged)
    ));
    session.detach().unwrap();
    assert!(matches!(
        client
            .seeded_style_recommendations("1", handle(&feed["value"], 0))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls().len(), 3);
}
