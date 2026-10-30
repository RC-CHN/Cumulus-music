use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchDiscoveryQuery as Q, SearchHistoryCommand as H},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    invalid: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            if self.invalid.load(Ordering::SeqCst) {
                return Ok(json!({"code":200,"data":{"unexpected":[]}}));
            }
            let data = match path {
                "/api/search/default/keyword/get" => {
                    json!({"refreshTime":1,"keywords":[{"showKeyword":"显示词","realkeyword":"实际词","bizQueryInfo":"PRIVATE"}]})
                }
                "/api/search/pc/rcmd/keyword/get" => {
                    json!({"operateWords":{"first":{"keyword":"运营词"}},"algWords":[{"keyword":"推荐词","toUserWord":"推荐显示","url":"https://private.invalid"}]})
                }
                "/api/search/pc/chart/list" => {
                    json!({"tabInfo":{"list":[{"id":"HOT_SEARCH_SONG#@#","name":"热搜榜"},{"id":"unknown","name":"不可用分支"}]},"items":{"HOT_SEARCH_SONG#@#":[{"searchWord":"热词","toUserWord":"长标题","iconType":1,"url":"PRIVATE"}]}})
                }
                "/api/search/pc/chart/detail" => {
                    json!({"itemList":[{"searchWord":"榜单补读","toUserWord":"榜单补读"}]})
                }
                "/api/search/pc/suggest/keyword/get" => {
                    json!({"recTitle":"猜你想搜","recs":[{"keyword":"推荐建议"}],"suggests":[{"keyword":"联想词","name":"显示名称","cookie":"PRIVATE"}]})
                }
                _ => return Err(Error::Response("unexpected search fixture request")),
            };
            Ok(json!({"code":200,"data":data}))
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
    Arc<cumulus_core::storage::StateStore>,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(Backend {
        calls: Mutex::default(),
        invalid: AtomicBool::new(false),
        hold: AtomicBool::new(false),
        started: Notify::new(),
        release: Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (root, api, session, client, store)
}
#[tokio::test]
async fn official_pc_search_shapes_remain_typed_and_do_not_expose_tracking_or_urls() {
    let (_r, api, session, client, _) = fixture();
    let e = session.token().unwrap().generation.to_string();
    let queries = [
        Q::Defaults,
        Q::Recommendations {
            last_query: " 上次输入 ".into(),
        },
        Q::Charts,
        Q::Chart {
            id: "HOT_SEARCH_SONG#@#".into(),
        },
        Q::Suggestions {
            keyword: " 组合词 ".into(),
        },
    ];
    let mut values = vec![];
    for query in queries {
        values
            .push(serde_json::to_value(client.search_discovery(&e, query).await.unwrap()).unwrap());
    }
    assert_eq!(values[0]["value"]["words"][0]["keyword"], "实际词");
    assert_eq!(values[0]["value"]["refreshMs"], 2000);
    assert_eq!(values[1]["value"]["words"].as_array().unwrap().len(), 2);
    assert_eq!(values[2]["value"]["charts"].as_array().unwrap().len(), 1);
    assert_eq!(values[4]["value"]["suggestions"][0]["label"], "显示名称");
    let encoded = serde_json::to_string(&values).unwrap();
    assert!(
        !encoded.contains("PRIVATE")
            && !encoded.contains("https://")
            && !encoded.contains("bizQueryInfo")
    );
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls[4],
        (
            "/api/search/pc/suggest/keyword/get".into(),
            json!({"keyword":"组合词"})
        )
    );
    assert_eq!(calls[1].1, json!({"lastQuery":"上次输入"}));
}
#[test]
fn history_is_ordered_bounded_durable_and_account_isolated() {
    let (_r, api, session, client, store) = fixture();
    let e = session.token().unwrap().generation.to_string();
    for i in 0..35 {
        client
            .search_history(
                &e,
                H::Remember {
                    keyword: format!("词{i}"),
                },
            )
            .unwrap();
    }
    let value = client
        .search_history(
            &e,
            H::Remember {
                keyword: " 词20 ".into(),
            },
        )
        .unwrap()
        .value;
    assert_eq!(value.len(), 30);
    assert_eq!(value[0], "词20");
    client
        .search_history(
            &e,
            H::Remove {
                keyword: "词20".into(),
            },
        )
        .unwrap();
    assert_eq!(client.search_history(&e, H::Read).unwrap().value.len(), 29);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let next = session.token().unwrap().generation.to_string();
    assert!(client.search_history(&e, H::Clear).is_err());
    assert!(
        client
            .search_history(&next, H::Read)
            .unwrap()
            .value
            .is_empty()
    );
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let again = session.token().unwrap().generation.to_string();
    assert_eq!(
        client.search_history(&again, H::Read).unwrap().value.len(),
        29
    );
    store
        .put(
            "9",
            "search-history-v1",
            "keywords",
            &json!({"broken":true}),
        )
        .unwrap();
    assert!(client.search_history(&again, H::Read).is_err());
    assert!(
        client
            .search_history(&again, H::Clear)
            .unwrap()
            .value
            .is_empty()
    );
    assert!(api.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn malformed_search_is_not_an_empty_success_and_late_account_results_are_rejected() {
    let (_r, api, session, client, _) = fixture();
    let e = session.token().unwrap().generation.to_string();
    api.invalid.store(true, Ordering::SeqCst);
    assert!(client.search_discovery(&e, Q::Charts).await.is_err());
    api.invalid.store(false, Ordering::SeqCst);
    api.hold.store(true, Ordering::SeqCst);
    let request = client.search_discovery(&e, Q::Defaults);
    tokio::pin!(request);
    tokio::select! {v=&mut request=>panic!("unexpected completion: {}",v.is_ok()),_ = api.started.notified()=>{}}
    session.detach().unwrap();
    api.release.notify_one();
    assert!(request.await.is_err());
}
