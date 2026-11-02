use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::CommentResource},
    comments::ResourceKind,
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

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    wrong: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args.clone()));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            let mut id = args["id"].clone();
            if path == "/api/v3/song/detail" {
                let rows: Vec<Value> = serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                id = rows[0]["id"].clone();
            }
            if self.wrong.load(Ordering::SeqCst) {
                id = json!("999");
            }
            Ok(match path {
                "/api/v3/song/detail" => {
                    json!({"code":200,"songs":[{"id":id,"name":"Song","ar":[{"id":"900719925474099312345","name":"Linked"},{"id":0,"name":"Plain"}],"al":{"id":7,"name":"Album","picUrl":"http://p1.music.126.net/a?token=PRIVATE"},"cookie":"PRIVATE","url":"https://example.invalid/PRIVATE"}]})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":{"id":id,"name":"Voice","mainTrackId":181,"coverUrl":"https://p1.music.126.net/a?token=PRIVATE","radio":{"id":71,"name":"Radio"},"dj":{"userId":3,"nickname":"Creator","cookie":"PRIVATE"},"url":"https://example.invalid/PRIVATE"}})
                }
                "/api/mlog/detail/v1" => {
                    json!({"code":200,"data":{"id":id,"isMv":true,"resource":{"mlogBaseData":{"type":3,"text":"Video","coverUrl":"http://p1.music.126.net/a?token=PRIVATE"},"mlogExtVO":{"artists":[{"id":12,"name":"Artist"}]},"authorization":"PRIVATE"}}})
                }
                _ => return Err(Error::Response("unexpected metadata request")),
            })
        })
    }
}
fn make(
    root: &std::path::Path,
    api: Arc<Backend>,
    uid: u64,
) -> (
    Arc<AccountSession>,
    Arc<MusicClient>,
    Arc<cumulus_core::storage::StateStore>,
) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (session, client, store)
}
fn resource(kind: ResourceKind, id: &str) -> CommentResource {
    CommentResource {
        kind,
        id: id.into(),
        owner: None,
    }
}

#[tokio::test]
async fn official_card_kinds_project_only_public_metadata_and_persist_per_account() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client, _) = make(root.path(), api.clone(), 9);
    let targets = [
        resource(ResourceKind::Track, "900719925474099312345"),
        resource(ResourceKind::Voice, "81"),
        resource(ResourceKind::Mv, "123"),
    ];
    for r in &targets {
        let value = client
            .comments_resource("1", r.clone(), true)
            .await
            .unwrap();
        let data = serde_json::to_value(value).unwrap();
        assert!(!data.to_string().contains("PRIVATE"));
        assert_eq!(data["value"]["resource"]["id"], r.id);
        assert_eq!(data["value"]["cached"], false);
        assert_eq!(
            data["value"]["card"]["kind"],
            serde_json::to_value(r.kind).unwrap()
        );
    }
    let calls = api.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].1["c"], r#"[{"id":"900719925474099312345","v":0}]"#);
    assert_eq!(calls[1].1, json!({"id":"81"}));
    assert_eq!(calls[2].1, json!({"id":"123","type":"2"}));
    drop(client);
    let (_, reopened, _) = make(root.path(), api.clone(), 9);
    for r in &targets {
        assert!(
            reopened
                .comments_resource("1", r.clone(), false)
                .await
                .unwrap()
                .value
                .cached
        );
    }
    assert_eq!(api.calls.lock().unwrap().len(), 3);
    let (_, other, _) = make(root.path(), api.clone(), 10);
    assert!(
        other
            .comments_resource("1", targets[0].clone(), false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn wrong_resource_and_cache_identity_are_rejected_without_corrupting_previous_metadata() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_session, client, store) = make(root.path(), api.clone(), 9);
    let r = resource(ResourceKind::Track, "1");
    client
        .comments_resource("1", r.clone(), true)
        .await
        .unwrap();
    api.wrong.store(true, Ordering::SeqCst);
    for r in [
        r.clone(),
        resource(ResourceKind::Voice, "81"),
        resource(ResourceKind::Mv, "123"),
    ] {
        assert!(client.comments_resource("1", r, true).await.is_err());
    }
    assert!(
        client
            .comments_resource("1", r.clone(), false)
            .await
            .is_ok()
    );

    let mut saved = store
        .get("9", "comment-resource-v1", &r.thread().unwrap())
        .unwrap()
        .unwrap();
    saved["resource"]["id"] = json!("2");
    store
        .put("9", "comment-resource-v1", &r.thread().unwrap(), &saved)
        .unwrap();
    assert!(client.comments_resource("1", r, false).await.is_err());
}

#[tokio::test]
async fn unsupported_official_card_branches_do_not_invent_network_requests() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client, _) = make(root.path(), api.clone(), 9);
    for r in [
        resource(ResourceKind::Album, "7"),
        resource(ResourceKind::Playlist, "8"),
        resource(ResourceKind::Video, "AB12"),
    ] {
        assert!(
            client
                .comments_resource("1", r, true)
                .await
                .unwrap()
                .value
                .card
                .is_none()
        );
    }
    assert!(api.calls.lock().unwrap().is_empty());
    assert!(matches!(
        client
            .comments_resource("0", resource(ResourceKind::Track, "1"), true)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn account_replacement_rejects_late_metadata_and_does_not_seed_the_new_cache() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client, _) = make(root.path(), api.clone(), 9);
    api.hold.store(true, Ordering::SeqCst);
    let other = client.clone();
    let request = tokio::spawn(async move {
        other
            .comments_resource("1", resource(ResourceKind::Track, "1"), true)
            .await
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(request.await.unwrap(), Err(Error::StaleResponse)));
    let epoch = session.token().unwrap().generation.to_string();
    assert!(
        client
            .comments_resource(&epoch, resource(ResourceKind::Track, "1"), false)
            .await
            .is_err()
    );
}
