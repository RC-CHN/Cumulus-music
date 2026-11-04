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
