use cumulus_core::{
    Error,
    account::AccountSession,
    comments::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
fn item(id: &str, parent: Option<&str>) -> Value {
    json!({"commentId":id,"parentCommentId":parent.unwrap_or("0"),"content":"plain floor text","time":1700000000000u64,"liked":false,"likedCount":0,"user":{"userId":"1","nickname":"Example"},"beReplied":[]})
}
fn response() -> Value {
    json!({"code":200,"data":{"ownerComment":item("101",None),"comments":[item("900719925474099312345",Some("101"))],"bestComments":[],"currentComment":null,"hasMore":true,"totalCount":2,"time":1700000000000u64}})
}
struct Server {
    response: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Server {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            response: Mutex::new(response()),
            calls: Mutex::default(),
            hold: AtomicBool::new(false),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}
impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/resource/comment/floor/get");
            assert!(!cache);
            self.calls.lock().unwrap().push((path.into(), payload));
            let value = self.response.lock().unwrap().clone();
            if self.hold.swap(false, Ordering::AcqRel) {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(value)
        })
    }
}
fn resource() -> CommentResource {
    CommentResource {
        kind: ResourceKind::Track,
        id: "123".into(),
        owner: None,
    }
}
#[tokio::test]
async fn floor_preserves_parent_identity_large_ids_and_server_time_cursor_without_guessing_offsets()
{
    let server = Server::new();
    let api = CommentApi::new(server.clone());
    let q = CommentFloorQuery::first("101".into());
    let first = api.floor(&resource(), &q).await.unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.comments[0].id, "900719925474099312345");
    assert_eq!(first.comments[0].parent_id.as_deref(), Some("101"));
    assert_eq!(
        server.calls.lock().unwrap()[0].1,
        json!({"threadId":"R_SO_4_123","parentCommentId":"101","time":-1,"limit":20})
    );
    let next = first.next.unwrap();
    assert_eq!(next.time, 1700000000000);
    let mut value = response();
    value["data"]["hasMore"] = json!(false);
    value["data"]["comments"] = json!([item("900719925474099312346", Some("101"))]);
    *server.response.lock().unwrap() = value;
    let second = api.floor(&resource(), &next).await.unwrap();
    assert!(second.next.is_none());
    assert_eq!(server.calls.lock().unwrap()[1].1["time"], next.time);
}
#[tokio::test]
async fn floor_refuses_wrong_parent_unrelated_current_comments_stalled_or_unbounded_pages() {
    let server = Server::new();
    let api = CommentApi::new(server.clone());
    let q = CommentFloorQuery::first("101".into());
    for invalid in [
        json!({"ownerComment":item("102",None)}),
        json!({"comments":[item("201",Some("999"))]}),
        json!({"currentComment":item("201",Some("999"))}),
        json!({"comments":[]}),
        json!({"time":-1}),
        json!({"hasMore":null}),
        json!({"comments":[item("201",Some("101")),item("201",Some("101"))]}),
    ] {
        let mut value = response();
        value["data"]
            .as_object_mut()
            .unwrap()
            .extend(invalid.as_object().unwrap().clone());
        *server.response.lock().unwrap() = value;
        assert!(api.floor(&resource(), &q).await.is_err());
    }
    let count = server.calls.lock().unwrap().len();
    for query in [
        CommentFloorQuery {
            limit: 101,
            ..q.clone()
        },
        CommentFloorQuery {
            time: 0,
            ..q.clone()
        },
        CommentFloorQuery {
            time: 9_007_199_254_740_992,
            ..q.clone()
        },
        CommentFloorQuery {
            parent_id: "../x".into(),
            ..q
        },
    ] {
        assert!(api.floor(&resource(), &query).await.is_err());
    }
    assert_eq!(server.calls.lock().unwrap().len(), count);
}
#[tokio::test]
async fn floor_cache_survives_reopen_but_not_wrong_account_or_a_new_write_revision() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let server = Server::new();
    let session = Arc::new(AccountSession::new(server.clone(), Some(store.clone())));
    session
        .adopt(
            server.clone(),
            json!({"userId":"1"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let t = session.token().unwrap();
    let service = Arc::new(Comments::new(session.clone()).unwrap());
    let r = resource();
    let q = CommentFloorQuery::first("101".into());
    service.floor_for(&t, &r, &q).await.unwrap();
    let restored = Comments::new(session.clone()).unwrap();
    assert!(restored.cached_floor_for(&t, &r, &q).unwrap().is_some());
    assert_eq!(server.calls.lock().unwrap().len(), 1);
    server.hold.store(true, Ordering::Release);
    let s = service.clone();
    let token = t.clone();
    let query = q.clone();
    let resource = r.clone();
    let page = tokio::spawn(async move { s.floor_for(&token, &resource, &query).await });
    server.started.notified().await;
    service
        .prepare_write_for(
            &t,
            &r,
            CommentWriteCommand::Publish {
                content: "draft only; never sent".into(),
                reply_to: None,
            },
        )
        .await
        .unwrap();
    server.release.notify_one();
    assert!(matches!(page.await.unwrap(), Err(Error::QueueChanged)));
    assert!(restored.cached_floor_for(&t, &r, &q).unwrap().is_none());
    session
        .adopt(server.clone(), json!({"userId":"2"}), &t)
        .unwrap();
    assert!(service.cached_floor_for(&t, &r, &q).is_err());
    assert!(
        service
            .cached_floor_for(&session.token().unwrap(), &r, &q)
            .unwrap()
            .is_none()
    );
}
