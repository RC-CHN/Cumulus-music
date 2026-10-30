use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchCategory as C, SearchResultQuery},
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
    reply: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
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
            Ok(self.reply.lock().unwrap().clone())
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend {
        reply: Mutex::new(json!({"code":200})),
        calls: Mutex::default(),
        hold: AtomicBool::new(false),
        started: Notify::new(),
        release: Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (root, api, session, client)
}
fn query(category: C) -> SearchResultQuery {
    SearchResultQuery {
        keyword: " 合成 ".into(),
        category,
        offset: 0,
        limit: 30,
        correct: true,
    }
}
#[tokio::test]
async fn categories_use_official_shapes_and_only_public_card_fields_cross_ipc() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let payloads: Value =
        serde_json::from_str(include_str!("fixtures/search-results.json")).unwrap();
    for category in C::ALL {
        let key = serde_json::to_value(category).unwrap();
        *api.reply.lock().unwrap() = payloads[key.as_str().unwrap()].clone();
        let view = client
            .search_results(&epoch, query(category))
            .await
            .unwrap()
            .value;
        assert!(!view.sections.is_empty());
        assert!(!view.sections[0].items.is_empty());
        assert!(!serde_json::to_string(&view).unwrap().contains("PRIVATE"));
        let calls = api.calls.lock().unwrap();
        let (path, args) = calls.last().unwrap();
        assert!(!path.contains("cloudsearch"));
        if matches!(category, C::Playlists | C::Artists | C::Albums | C::Users) {
            assert_eq!(args["s"], "合成");
            assert_eq!(args["queryCorrect"], true);
        } else {
            assert_eq!(args["keyword"], "合成");
            assert_eq!(args["scene"], "normal");
        }
        if category == C::Songs {
            assert!(view.more);
            assert_eq!(view.next_offset, 3);
        }
        if category == C::Lyrics {
            assert!(!serde_json::to_string(&view).unwrap().contains("<em>"));
        }
    }
}
#[tokio::test]
async fn contradictory_empty_counts_stop_paging_and_malformed_results_are_errors() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() =
        json!({"code":200,"data":{"resources":[],"totalCount":46,"hasMore":true}});
    let result = client
        .search_results(&epoch, query(C::Mvs))
        .await
        .unwrap()
        .value;
    assert_eq!(result.total, Some(46));
    assert!(!result.more);
    assert!(result.notice.is_some());
    *api.reply.lock().unwrap() = json!({"code":200,"data":{}});
    assert!(
        client
            .search_results(&epoch, query(C::Songs))
            .await
            .is_err()
    );
    let before = api.calls.lock().unwrap().len();
    let mut invalid = query(C::All);
    invalid.offset = 30;
    assert!(client.search_results(&epoch, invalid).await.is_err());
    assert_eq!(api.calls.lock().unwrap().len(), before);
}
#[tokio::test]
async fn result_from_replaced_account_is_rejected() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[],"totalCount":0}});
    api.hold.store(true, Ordering::SeqCst);
    let request = client.search_results(&epoch, query(C::Songs));
    tokio::pin!(request);
    tokio::select! {_=&mut request=>panic!("request must wait"),_=api.started.notified()=>{}}
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(request.await.is_err());
}
