use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::CommunityFeedAction},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

const FEED: &str = "/api/event/pc/history/feed/get";
const DETAIL: &str = "/api/v1/event/73/908000000000000001";
const COMMENTS: &str = "/api/v1/resource/comments/A_EV_2_908000000000000001_73";
const CURSOR: &str = "90071992547409938888";

fn event(anonymous: bool) -> Value {
    json!({"id":"908000000000000001","type":18,"eventTime":1700000000000_u64,
        "user":{"userId":73,"nickname":"合成发布者","avatarUrl":"https://p1.music.126.net/cover.jpg?PRIVATE=1","signature":"PRIVATE"},
        "info":{"commentCount":2,"likedCount":5,"threadId":"A_EV_2_908000000000000001_73",
            "latestLikedUsers":[{"userId":19,"nickname":"PRIVATE"}]},
        "privacySettingInfo":{"credential":"PRIVATE"},
        "anonymityInfo":if anonymous {json!({"encryptUserId":"PRIVATE"})} else {Value::Null},
        "pics":[{"originUrl":"https://p1.music.126.net/event.jpg?auth=PRIVATE"},
            {"originUrl":"https://invalid.example/PRIVATE"}],
        "json":json!({"title":"合成笔记","msg":"测试动态","song":{"id":19,"name":"合成歌曲",
            "url":"https://example.invalid/PRIVATE"},"credentials":"PRIVATE"}).to_string()})
}

#[derive(Default)]
struct Fixture {
    calls: Mutex<Vec<(String, Value)>>,
    detail_mode: AtomicUsize,
    next_count: AtomicUsize,
    hold: AtomicBool,
    hold_comment: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls.lock().unwrap().push((path.into(), body.clone()));
            if (self.hold.load(Ordering::SeqCst) && path == FEED)
                || (self.hold_comment.load(Ordering::SeqCst) && path == COMMENTS)
            {
                self.started.notify_one();
                self.release.notified().await;
            }
            match path {
                FEED if body == json!({"cursor":"-1"}) => Ok(json!({"code":200,"data":{
                    "cursor":CURSOR,"more":true,"events":[event(false),
                        {"id":9082,"type":21,"user":{"userId":8,"nickname":"不支持作者"},"json":"{\"msg\":\"PRIVATE\"}"},
                        {"id":9083,"type":22,"user":{"userId":8,"nickname":"已删除作者"},"json":"{}"}]}})),
                FEED if body == json!({"cursor":-1}) => Ok(json!({"code":200,"data":{
                    "cursor":7000,"more":true,"events":[event(false)]}})),
                FEED if body == json!({"cursor":CURSOR}) => {
                    if self.next_count.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Ok(
                            json!({"code":200,"data":{"cursor":CURSOR,"more":true,"events":[]}}),
                        );
                    }
                    Ok(json!({"code":200,"data":{"cursor":0,"more":false,"events":[event(true)]}}))
                }
                DETAIL if body == json!({}) => {
                    let mut row = event(false);
                    match self.detail_mode.load(Ordering::SeqCst) {
                        1 => row["user"]["userId"] = 74.into(),
                        2 => return Ok(json!({"code":403})),
                        3 => row["info"]["threadId"] = "A_EV_2_11_77".into(),
                        _ => (),
                    }
                    Ok(json!({"code":200,"event":row}))
                }
                COMMENTS => Ok(json!({"code":200,"more":false,"total":1,"comments":[{
                    "commentId":11,"likedCount":0,"content":"只读评论","time":1700000000001_u64,
                    "user":{"userId":77,"nickname":"合成评论作者","avatarUrl":"https://invalid.example/PRIVATE"}}]})),
                _ => Err(Error::Response("unexpected community test request")),
            }
        })
    }
}

fn fixture() -> (
    tempfile::TempDir,
    Arc<AccountSession>,
    Arc<Fixture>,
    Arc<MusicClient>,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Fixture::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (root, session, api, client)
}

fn next(continuation: &str) -> CommunityFeedAction {
    CommunityFeedAction::Next {
        continuation: continuation.into(),
    }
}

#[tokio::test]
async fn exact_feed_refresh_detail_and_comment_reads_keep_raw_identity_native() {
    let (_dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let first = serde_json::to_value(
        client
            .community_feed(&epoch, CommunityFeedAction::First)
            .await
            .unwrap(),
    )
    .unwrap();
    let key = first["value"]["cards"][0]["detail"].as_str().unwrap();
    let continuation = first["value"]["continuation"].as_str().unwrap();
    assert_eq!(first["value"]["cards"][0]["attachment"]["kind"], "track");
    assert_eq!(first["value"]["cards"][0]["attachment"]["id"], "19");
    assert_eq!(first["value"]["cards"][1]["status"], "unsupported");
    assert_eq!(first["value"]["cards"][2]["status"], "deleted");
    assert!(first["value"]["cards"][1]["detail"].is_null());
    for private in [
        "PRIVATE",
        CURSOR,
        "908000000000000001",
        "threadId",
        "privacySettingInfo",
    ] {
        assert!(!first.to_string().contains(private), "{private}");
    }
    assert!(matches!(
        client.community_comments(&epoch, key, 0).await,
        Err(Error::StaleResponse)
    ));
    let detail = serde_json::to_value(client.community_detail(&epoch, key).await.unwrap()).unwrap();
    assert_eq!(detail["value"]["commentsAvailable"], true);
    let comments =
        serde_json::to_value(client.community_comments(&epoch, key, 0).await.unwrap()).unwrap();
    assert_eq!(comments["value"]["comments"][0]["text"], "只读评论");
    assert!(!comments.to_string().contains("profileId"));
    assert!(!comments.to_string().contains("PRIVATE"));
    api.detail_mode.store(1, Ordering::SeqCst);
    assert!(matches!(
        client.community_detail(&epoch, key).await,
        Err(Error::Response(_))
    ));
    assert!(matches!(
        client.community_comments(&epoch, key, 0).await,
        Err(Error::StaleResponse)
    ));
    api.detail_mode.store(2, Ordering::SeqCst);
    assert!(
        client
            .community_detail(&epoch, key)
            .await
            .unwrap()
            .value
            .is_none()
    );
    api.detail_mode.store(3, Ordering::SeqCst);
    let detail = serde_json::to_value(client.community_detail(&epoch, key).await.unwrap()).unwrap();
    assert_eq!(detail["value"]["commentsAvailable"], false);
    assert!(matches!(
        client.community_comments(&epoch, key, 0).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.community_feed(&epoch, next(continuation)).await,
        Err(Error::Response(_))
    ));
    let after = serde_json::to_value(
        client
            .community_feed(&epoch, next(continuation))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(after["value"]["cards"][0]["key"], key);
    assert_eq!(after["value"]["cards"][0]["authorName"], "匿名用户");
    assert!(after["value"]["cards"][0]["detail"].is_null());
    assert!(after["value"]["cards"][0]["avatarUrl"].is_null());
    assert!(matches!(
        client.community_detail(&epoch, key).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.community_feed(&epoch, next(continuation)).await,
        Err(Error::StaleResponse)
    ));

    let refresh = serde_json::to_value(
        client
            .community_feed(&epoch, CommunityFeedAction::Refresh)
            .await
            .unwrap(),
    )
    .unwrap();
    let restart = refresh["value"]["continuation"].as_str().unwrap();
    let replaced =
        serde_json::to_value(client.community_feed(&epoch, next(restart)).await.unwrap()).unwrap();
    assert_eq!(replaced["value"]["replace"], true);
    let calls = api.calls.lock().unwrap();
    let observed_feed: Vec<_> = calls
        .iter()
        .filter(|(path, _)| path == FEED)
        .map(|(_, body)| body.clone())
        .collect();
    assert_eq!(
        observed_feed,
        [
            json!({"cursor":"-1"}),
            json!({"cursor":CURSOR}),
            json!({"cursor":CURSOR}),
            json!({"cursor":-1}),
            json!({"cursor":"-1"})
        ]
    );
    assert_eq!(calls.iter().filter(|(path, _)| path == DETAIL).count(), 4);
    let comment_calls: Vec<_> = calls.iter().filter(|(path, _)| path == COMMENTS).collect();
    assert_eq!(comment_calls.len(), 1);
    assert_eq!(
        comment_calls[0].1,
        json!({"limit":60,"offset":0,"beforeTime":0,
        "compareUserLocation":false,"commentId":"0","composeConcert":false,
        "markReplied":false,"forceFlatComment":false,"showInner":false})
    );
}

#[tokio::test]
async fn account_switch_rejects_inflight_results_and_foreign_handles_before_the_wire() {
    let (_dir, session, api, client) = fixture();
    let old = session.token().unwrap().generation.to_string();
    let baseline = serde_json::to_value(
        client
            .community_feed(&old, CommunityFeedAction::First)
            .await
            .unwrap(),
    )
    .unwrap();
    let key = baseline["value"]["cards"][0]["detail"].as_str().unwrap();
    let continuation = baseline["value"]["continuation"].as_str().unwrap();
    api.hold.store(true, Ordering::SeqCst);
    let worker = client.clone();
    let old_epoch = old.clone();
    let read = tokio::spawn(async move {
        worker
            .community_feed(&old_epoch, CommunityFeedAction::Refresh)
            .await
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let new = session.token().unwrap().generation.to_string();
    api.release.notify_one();
    assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(
        client.community_detail(&new, key).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.community_feed(&new, next(continuation)).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .community_feed(&old, CommunityFeedAction::First)
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_new_detail_receipt_retires_pending_comments_for_the_same_event() {
    let (_dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let feed = serde_json::to_value(
        client
            .community_feed(&epoch, CommunityFeedAction::First)
            .await
            .unwrap(),
    )
    .unwrap();
    let key = feed["value"]["cards"][0]["detail"]
        .as_str()
        .unwrap()
        .to_owned();
    client.community_detail(&epoch, &key).await.unwrap();
    api.hold_comment.store(true, Ordering::SeqCst);
    let worker = client.clone();
    let old_epoch = epoch.clone();
    let old_key = key.clone();
    let request =
        tokio::spawn(async move { worker.community_comments(&old_epoch, &old_key, 0).await });
    api.started.notified().await;
    client.community_detail(&epoch, &key).await.unwrap();
    api.release.notify_one();
    assert!(matches!(request.await.unwrap(), Err(Error::StaleResponse)));
}
