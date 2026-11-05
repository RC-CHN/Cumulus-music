use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchCategory, SearchResultQuery},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

type Expected = (&'static str, Value, Value);

struct Script(Mutex<VecDeque<Expected>>);
impl Api for Script {
    fn request<'a>(&'a self, path: &'a str, data: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            let (expected, body, response) = self.0.lock().unwrap().pop_front().expect(path);
            assert_eq!((path, data), (expected, body));
            Ok(response)
        })
    }
}
const EXP: &str = "/api/rtrs/abt/front/expinfo/list";
const TABS: &str = "/api/search/pc/result/tab";
const LEGACY: &str = "/api/search/pc/complex/page";
const V3: &str = "/api/search/pc/complex/page/v3";
const WORD: &str = "合成";

fn query() -> SearchResultQuery {
    SearchResultQuery {
        keyword: WORD.into(),
        category: SearchCategory::All,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    }
}
fn assign(group: &str, expanded: &str, dense: &str) -> Expected {
    (
        EXP,
        json!({"expNames":"PH-PC-searchHot,PH-PC-searchMore2,PH-PC-searchPageEfficiency"}),
        json!({"code":200,"data":[{"expName":"PH-PC-searchHot","expGroupName":group},
          {"expName":"PH-PC-searchMore2","expGroupName":expanded},
          {"expName":"PH-PC-searchPageEfficiency","expGroupName":dense}]}),
    )
}
fn tabs(response: Value) -> Expected {
    (TABS, json!({"keyword":WORD}), response)
}
fn v3(response: Value) -> Expected {
    (
        V3,
        json!({"keyword":WORD,"scene":"normal","needCorrect":true,"channel":"suggest","bizQueryInfo":""}),
        response,
    )
}
fn legacy() -> Expected {
    (
        LEGACY,
        json!({"keyword":WORD,"scene":"normal","needCorrect":true,"channel":"suggest","cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""}),
        json!({"code":200,"data":{"blocks":[]}}),
    )
}

#[tokio::test]
async fn generic_search_assignment_cannot_grant_the_independent_signed_footer() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let mut unverified = assign("c", "t1", "t1");
    unverified.2["data"]
        .as_array_mut()
        .unwrap()
        .push(json!({"expName":"PH-PC-search-result","expGroupName":"t1"}));
    let mut result = legacy();
    result.2 = json!({"code":200,"data":{"blocks":[{
        "blockCode":"search_block_song","resources":[{"type":"song","baseInfo":{
            "id":3,"name":"普通曲目","extInfo":{"songAlias":"无授权别名",
                "algClickableTags":[{"text":"无授权入口","reasonType":1,"reasonId":"r-9",
                    "resourceId":3,"clickable":true}]}
        }}]
    }]}});
    let api = Arc::new(Script(Mutex::new(VecDeque::from([unverified, result]))));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session, registry).with_official_search_experiment();
    let view = serde_json::to_value(
        client
            .search_results(&client.account().unwrap().epoch, query())
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(view["previewLayout"], "compact_carousel");
    let song = &view["sections"][0]["items"][0];
    assert_eq!(song["track"]["id"], "3");
    assert!(song.get("hint").is_none());
    assert!(song.get("reasons").is_none());
    assert!(api.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_search_assignment_tabs_fallback_and_account_fence_follow_the_recovered_routes() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Script(Mutex::new(VecDeque::from([
        assign("t1", "c", "t1"),
        tabs(
            json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"视频"},{"title":"单曲"},{"title":"unknown"}]}}),
        ),
        v3(json!({"code":200,"data":{"blocks":[]}})),
        tabs(json!({"code":500})),
        v3(json!({"code":405,"msg":"综合说明"})),
        // A second owner must not reuse the first owner's t1 assignment or tabs.
        assign("t2", "t1", "t1"),
        legacy(),
    ]))));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let client =
        MusicClient::with_session(session.clone(), registry).with_official_search_experiment();
    let epoch = client.account().unwrap().epoch;
    let invalid = SearchResultQuery {
        offset: 1,
        ..query()
    };
    assert!(client.search_results(&epoch, invalid).await.is_err());
    let first = client.search_results(&epoch, query()).await.unwrap().value;
    assert!(first.preview_layout.is_none());
    assert_eq!(
        first.tabs,
        Some(vec![
            SearchCategory::All,
            SearchCategory::Mvs,
            SearchCategory::Songs
        ])
    );
    let retry = client.search_results(&epoch, query()).await.unwrap().value;
    assert_eq!(retry.tabs, first.tabs);
    assert_eq!(retry.empty_title.as_deref(), Some("综合说明"));
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    assert!(client.search_results(&epoch, query()).await.is_err());
    let second = client
        .search_results(&client.account().unwrap().epoch, query())
        .await
        .unwrap()
        .value;
    assert!(second.tabs.is_none());
    assert_eq!(
        serde_json::to_value(second).unwrap()["previewLayout"],
        "compact_carousel"
    );
    assert!(api.0.lock().unwrap().is_empty());
}

struct PreviewConfig;
impl cumulus_core::protocol::mconfig::ConfigBackend for PreviewConfig {
    fn get<'a>(
        &'a self,
        url: &'a cumulus_core::protocol::mconfig::ConfigUrl,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let flag = if url.as_str().ends_with("on.data") {
                json!(true)
            } else if url.as_str().ends_with("malformed.data") {
                json!("true")
            } else {
                json!(false)
            };
            Ok(json!({"preload#isOpenNoCopyrightMV":flag}))
        })
    }
}

#[tokio::test]
async fn v3_replacement_mv_requires_verified_pc_config_and_exact_rights_metadata() {
    use cumulus_core::protocol::mconfig::{APPLICATION_KEY, PcConfig};
    use std::collections::BTreeMap;
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let alt = |id, flag| {
        json!({"type":3,"expInfo":{"mvPlay":flag},
        "mvInfo":{"id":id,"pic16v9Url":"https://p1.music.126.net/safe.jpg?token=PRIVATE",
          "url":"https://private.invalid/PRIVATE"}})
    };
    let original = |id, st, toast| {
        json!({"type":"song","baseInfo":{"simpleSongData":{
            "id":id,"name":"歌曲","mv":602,"privilege":{"st":st,"toast":toast},
            "noCopyrightRcmd":alt(json!("601"), json!(true))
        }}})
    };
    let mut changed = original(3, -200, "");
    changed["extInfo"]["noCopyrightRcmd"] = alt(json!(601), json!("true"));
    let mut invalid = original(6, -200, "");
    invalid["extInfo"]["noCopyrightRcmd"] =
        alt(json!("https://private.invalid/PRIVATE"), json!(true));
    let mut cleared = original(7, -200, "");
    cleared["extInfo"]["noCopyrightRcmd"] = Value::Null;
    let mut cover = original(8, -200, "");
    cover["extInfo"]["noCopyrightRcmd"] = alt(json!(601), json!(true));
    cover["extInfo"]["noCopyrightRcmd"]["mvInfo"]["pic16v9Url"] =
        json!("https://private.invalid/PRIVATE");
    let response = json!({"code":200,"data":{"preload#isOpenNoCopyrightMV":true,"blocks":[
        {"blockCode":"search_block_song","resources":[original(1,-200,""),
          original(2,-100,""), changed, original(4,-100,"down"), original(5,0,""), invalid, cleared, cover]}
    ]}});
    let mut script = VecDeque::from([assign("t1", "c", "c")]);
    for _ in 0..4 {
        script.push_back(tabs(
            json!({"code":200,"data":{"algTabs":[{"title":"综合"}]}}),
        ));
        script.push_back(v3(response.clone()));
    }
    let api = Arc::new(Script(Mutex::new(script)));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let config = PcConfig::new(registry.store(), Arc::new(PreviewConfig)).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_official_search_experiment()
        .with_native_search_preview_config(config.clone());
    let epoch = client.account().unwrap().epoch;
    for (version, file) in [(0, ""), (1, "on"), (2, "malformed"), (3, "off")] {
        if version != 0 {
            config
                .resolve(&BTreeMap::from([(
                    "MConfig-Info".into(),
                    json!({APPLICATION_KEY:{
                        "os":"pc","version":version,
                        "fullUrl":format!("https://d5.music.126.net/obj/fixture/{file}.data")
                    }})
                    .to_string(),
                )]))
                .await
                .unwrap();
        }
        let view =
            serde_json::to_value(client.search_results(&epoch, query()).await.unwrap().value)
                .unwrap();
        let items = view["sections"][0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 8);
        for (index, item) in items.iter().enumerate() {
            let replacement = version == 1 && matches!(index, 0 | 3 | 7);
            assert_eq!(
                item["replacementMv"] == true,
                replacement,
                "version {version}, row {index}"
            );
            assert_eq!(
                item["mvId"].as_str(),
                if replacement {
                    Some("601")
                } else if index == 4 {
                    Some("602")
                } else {
                    None
                }
            );
            if replacement && index != 7 {
                assert!(
                    item["track"]["coverUrl"]
                        .as_str()
                        .unwrap()
                        .starts_with("https://p1.music.126.net/safe.jpg")
                );
            } else {
                assert!(item["track"]["coverUrl"].is_null());
            }
        }
        assert!(!view.to_string().contains("PRIVATE"));
        assert!(!view.to_string().contains("noCopyrightRcmd"));
    }
    assert!(api.0.lock().unwrap().is_empty());
}
