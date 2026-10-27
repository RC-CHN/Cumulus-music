use cumulus_core::{
    Error, Result,
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        mconfig::*,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
enum Reply {
    Value(Result<Value>),
    Pending(oneshot::Receiver<Result<Value>>),
}
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    replies: Mutex<VecDeque<Reply>>,
}
impl ConfigBackend for Backend {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(url.as_str().into());
            let r = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected config GET");
            match r {
                Reply::Value(r) => r,
                Reply::Pending(p) => p.await.unwrap(),
            }
        })
    }
}
fn metadata(version: u64, os: &str, bucket: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
 ("MConfig-Info".into(),json!({APPLICATION_KEY:{"version":version,"os":os,"fullUrl":format!("https://d5.music.126.net/obj/fixture/config{version}.data")}}).to_string()),
 ("MConfig-Bucket".into(),bucket.into())])
}
#[tokio::test]
async fn config_center_matches_executed_official_module_92_and_reopens() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/mconfig-official.json")).unwrap();
    for scenario in fixture["scenarios"].as_array().unwrap() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let backend = Arc::new(Backend::default());
        let service = PcConfig::new(store.clone(), backend.clone()).unwrap();
        if let Some(local) = scenario.get("local") {
            let s = Snapshot {
                origin: cumulus_core::protocol::http::ApiOrigin::Pc,
                version: local["version"].as_u64().unwrap(),
                bucket: local["bucket"].as_str().unwrap().into(),
                data: Some(local["data"].clone()),
                source_url: Some("https://d5.music.126.net/obj/fixture/cached.data".into()),
                generation: 1,
            };
            store
                .put(
                    "transport",
                    "pc-mconfig-v1",
                    "pc",
                    &serde_json::to_value(s).unwrap(),
                )
                .unwrap();
        }
        let mut calls = 0;
        for (step, expected) in scenario["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(scenario["expected"].as_array().unwrap())
        {
            let target = expected["calls"].as_array().unwrap().len();
            if target > calls {
                backend.replies.lock().unwrap().push_back(Reply::Value(
                    if step["failure"] == true {
                        Err(Error::Transport("fixture".into()))
                    } else {
                        Ok(step["object"].clone())
                    },
                ));
            }
            let headers = BTreeMap::from([
                (
                    "MConfig-Info".into(),
                    json!({APPLICATION_KEY:step["info"]}).to_string(),
                ),
                (
                    "MConfig-Bucket".into(),
                    step["bucket"].as_str().unwrap_or("").into(),
                ),
            ]);
            let result = service.resolve(&headers).await;
            assert_eq!(result.is_err(), step["failure"] == true, "{scenario}");
            let actual = service.snapshot().unwrap();
            assert_eq!(actual.version, expected["version"].as_u64().unwrap());
            assert_eq!(actual.bucket, expected["bucket"].as_str().unwrap());
            assert_eq!(actual.data.unwrap_or(Value::Null), expected["data"]);
            assert_eq!(json!(*backend.calls.lock().unwrap()), expected["calls"]);
            assert_eq!(
                serde_json::from_str::<Value>(
                    &service
                        .snapshot()
                        .unwrap()
                        .request_header("3.1.42.205594")
                        .unwrap()
                )
                .unwrap(),
                expected["header"]
            );
            calls = target;
        }
        let reopened =
            PcConfig::new(Arc::new(StateStore::open(dir.path()).unwrap()), backend).unwrap();
        assert_eq!(
            serde_json::to_value(reopened.snapshot().unwrap()).unwrap(),
            serde_json::to_value(service.snapshot().unwrap()).unwrap()
        );
    }
}
async fn called(backend: &Backend, n: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while backend.calls.lock().unwrap().len() < n {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn in_flight_bucket_changes_cancel_and_stale_store_never_publish_partial_policy() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let backend = Arc::new(Backend::default());
    let service = PcConfig::new(store.clone(), backend.clone()).unwrap();
    let (tx, rx) = oneshot::channel();
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Pending(rx));
    let task = tokio::spawn({
        let s = service.clone();
        async move { s.resolve(&metadata(1, "pc", "a")).await }
    });
    called(&backend, 1).await;
    assert_eq!(
        service.resolve(&metadata(2, "pc", "b")).await.unwrap(),
        Update::InProgress
    );
    tx.send(Ok(json!({"first":true}))).unwrap();
    assert_eq!(task.await.unwrap().unwrap(), Update::Updated);
    assert_eq!(service.snapshot().unwrap().bucket, "b");
    let (_tx, rx) = oneshot::channel();
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Pending(rx));
    let task = tokio::spawn({
        let s = service.clone();
        async move { s.resolve(&metadata(2, "pc", "c")).await }
    });
    called(&backend, 2).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Value(Ok(json!({"afterCancel":true}))));
    assert_eq!(
        service.resolve(&metadata(3, "pc", "")).await.unwrap(),
        Update::Updated
    );
    let (tx, rx) = oneshot::channel();
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Pending(rx));
    let task = tokio::spawn({
        let s = service.clone();
        async move { s.resolve(&metadata(4, "pc", "")).await }
    });
    called(&backend, 4).await;
    let second = PcConfig::new(store, backend.clone()).unwrap();
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Value(Ok(json!({"newer":true}))));
    second.resolve(&metadata(5, "pc", "")).await.unwrap();
    tx.send(Ok(json!({"stale":true}))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(service.snapshot().unwrap().version, 5);
    let (_tx, rx) = oneshot::channel();
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Pending(rx));
    let task = tokio::spawn({
        let s = service.clone();
        async move { s.resolve(&metadata(6, "pc", "")).await }
    });
    called(&backend, 6).await;
    service.close().unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
    assert_eq!(second.snapshot().unwrap().version, 5);
}
struct Tokens {
    service: Arc<PcConfig>,
    backend: Arc<Backend>,
    calls: Mutex<usize>,
    change: bool,
}
impl TokenProvider for Tokens {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(business, "fixture");
            *self.calls.lock().unwrap() += 1;
            if self.change {
                self.backend
                    .replies
                    .lock()
                    .unwrap()
                    .push_back(Reply::Value(Ok(json!({}))));
                self.service.resolve(&metadata(2, "pc", "")).await?;
            }
            Ok("synthetic-fresh-token".into())
        })
    }
}
#[tokio::test]
async fn dynamic_policy_injects_tokens_only_after_verified_pc_update_and_rejects_changed_policy() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let service = PcConfig::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        backend.clone(),
    )
    .unwrap();
    let tokens = Arc::new(Tokens {
        service: service.clone(),
        backend: backend.clone(),
        calls: Mutex::new(0),
        change: false,
    });
    let context =
        RequestContext::from_pc(service.clone(), "3.1.42.205594", tokens.clone()).unwrap();
    assert!(
        !context
            .prepare("/api/write", &json!({}))
            .await
            .unwrap()
            .headers
            .contains_key("X-antiCheatToken")
    );
    assert_eq!(
        service.resolve(&metadata(1, "web", "a")).await.unwrap(),
        Update::NonPc
    );
    assert!(backend.calls.lock().unwrap().is_empty());
    let config =
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/write"]}]}});
    backend
        .replies
        .lock()
        .unwrap()
        .push_back(Reply::Value(Ok(config)));
    service.resolve(&metadata(1, "pc", "")).await.unwrap();
    let p = context.prepare("/api/write", &json!({})).await.unwrap();
    assert_eq!(p.payload["checkToken"], p.headers["X-antiCheatToken"]);
    assert_eq!(*tokens.calls.lock().unwrap(), 1);
    let changing = Arc::new(Tokens {
        service: service.clone(),
        backend: backend.clone(),
        calls: Mutex::new(0),
        change: true,
    });
    let context = RequestContext::from_pc(service.clone(), "3.1.42.205594", changing).unwrap();
    assert!(matches!(
        context.prepare("/api/write", &json!({})).await,
        Err(Error::StaleResponse)
    ));
    backend.replies.lock().unwrap().push_back(Reply::Value(Ok(
        json!({"yidun#paths":{"whiteList":[{"businessId":7}]}}),
    )));
    assert!(service.resolve(&metadata(3, "pc", "")).await.is_err());
    assert_eq!(service.snapshot().unwrap().version, 2);
}
#[test]
fn config_urls_cannot_redirect_credentials_or_accept_non_cdn_sources() {
    for url in [
        "http://d5.music.126.net/obj/a.data",
        "https://music.126.net.attacker.invalid/obj/a.data",
        "https://127.0.0.1/obj/a.data",
        "https://u:p@d5.music.126.net/obj/a.data",
        "https://d5.music.126.net:8443/obj/a.data",
        "https://d5.music.126.net/obj/a.js",
        "https://d5.music.126.net/obj/a.data?cookie=x",
    ] {
        assert!(ConfigUrl::parse(url).is_err(), "{url}");
    }
    assert!(ConfigUrl::parse("https://d5.music.126.net/obj/a.data").is_ok());
}
