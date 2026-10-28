use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{ClientError, MusicClient, dto::PageRequest},
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
    liked: AtomicBool,
    expired: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    requests: Mutex<Vec<(String, Value)>>,
}
fn song(id: &Value) -> Value {
    json!({"id":id,"name":"曲目","ar":[{"id":"9007199254740993","name":"歌手"}],"al":{"id":"56","name":"专辑","picUrl":"http://p1.music.126.net/cover.jpg?token=DO_NOT_EXPORT"},"dt":240123,"fee":1,"url":"https://private/signed?MUSIC_U=DO_NOT_EXPORT","cookie":"DO_NOT_EXPORT"})
}
fn playlist() -> Value {
    json!({"id":"7","userId":"9","name":"喜欢的音乐","specialType":5,"privacy":10,"creator":{"nickname":"用户"},"trackCount":999,"coverImgUrl":"http://p2.music.126.net/a.jpg","trackIds":[{"id":"1"},{"id":"2","v":-1},{"id":"3"},{"id":"4"}]})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if self.expired.load(Ordering::SeqCst) {
                return Ok(json!({"code":301,"cookie":"DO_NOT_EXPORT"}));
            }
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":9,"nickname":"用户","avatarUrl":"https://p1.music.126.net/a.jpg?secret=DO_NOT_EXPORT","token":"DO_NOT_EXPORT"}})
                }
                "/api/user/playlist" => json!({"code":200,"playlist":[playlist()],"more":false}),
                "/api/v6/playlist/detail" => json!({"code":200,"playlist":playlist()}),
                "/api/v3/song/detail" => {
                    let c: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":c.iter().filter(|v|v["id"]!="3").map(|v|song(&v["id"])).collect::<Vec<_>>()})
                }
                "/api/cloudsearch/pc" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    if payload["s"] == "empty" {
                        json!({"code":200,"result":{"songCount":0}})
                    } else {
                        json!({"code":200,"result":{"songCount":1,"songs":[song(&json!(1))]}})
                    }
                }
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":[song(&json!(1))]}})
                }
                "/api/song/like/get" => {
                    json!({"code":200,"ids":if self.liked.load(Ordering::SeqCst){vec![1]}else{vec![]}})
                }
                "/api/song/like" => {
                    self.liked
                        .store(payload["like"].as_bool().unwrap(), Ordering::SeqCst);
                    json!({"code":200})
                }
                _ => panic!("unexpected API {path}"),
            })
        })
    }
}
fn client(root: &std::path::Path, api: Arc<Backend>) -> (MusicClient, Arc<AccountSession>) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api, Some(registry.store())));
    (
        MusicClient::with_session(session.clone(), registry),
        session,
    )
}

#[tokio::test]
async fn boundary_preserves_authoritative_order_and_redacts_native_fields() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let account = client.restore_account().await.unwrap();
    let epoch = account.epoch.clone();
    let lists = client.playlists().await.unwrap();
    assert!(lists.value[0].favorite && lists.value[0].owned && lists.value[0].private);
    let first = client
        .playlist_page(
            "7",
            PageRequest {
                offset: 0,
                limit: 3,
                revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first.epoch, epoch);
    assert_eq!(first.value.total, 4);
    assert!(first.value.more);
    assert_eq!(
        first
            .value
            .tracks
            .iter()
            .map(|v| v.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert!(first.value.tracks[1].local && first.value.tracks[1].missing);
    assert_eq!(
        first.value.tracks[0].artist_refs[0].id.as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(first.value.tracks[0].album_id.as_deref(), Some("56"));
    assert!(first.value.tracks[1].artist_refs.is_empty());
    assert!(first.value.tracks[1].album_id.is_none());
    assert!(first.value.tracks[2].missing);
    let last = client
        .playlist_page(
            "7",
            PageRequest {
                offset: 3,
                limit: 100,
                revision: first.value.revision.clone(),
            },
        )
        .await
        .unwrap();
    assert!(!last.value.more);
    assert!(matches!(
        client
            .playlist_page(
                "7",
                PageRequest {
                    offset: 3,
                    limit: 100,
                    revision: Some("changed".into())
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(last.value.tracks[0].id, "4");
    let search = client
        .search_songs("test", PageRequest::default())
        .await
        .unwrap();
    let daily = client.daily_songs().await.unwrap();
    let empty = client
        .search_songs("empty", PageRequest::default())
        .await
        .unwrap();
    assert_eq!(empty.value.total, 0);
    assert!(empty.value.tracks.is_empty());
    let serialized = serde_json::to_string(
        &json!({"account":account,"lists":lists,"page":first,"search":search,"daily":daily}),
    )
    .unwrap();
    assert!(!serialized.contains("DO_NOT_EXPORT"));
    assert!(!serialized.contains("userId"));
    assert!(!serialized.contains("MUSIC_U"));
    assert!(!serialized.contains("private/signed"));
    assert!(serialized.contains("https://p1.music.126.net/cover.jpg?param=320y320"));
    assert!(
        client
            .search_songs(
                "test",
                PageRequest {
                    limit: 101,
                    offset: 0,
                    revision: None,
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn liked_boundary_uses_persistent_journal_and_readback() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    client.restore_account().await.unwrap();
    assert!(client.liked_ids().await.unwrap().value.is_empty());
    let added = client.set_liked("1", "1", true).await.unwrap();
    assert_eq!(added.value.observed, Some(true));
    assert!(matches!(
        added.value.phase,
        cumulus_core::account::resources::MutationPhase::Confirmed
    ));
    assert_eq!(client.liked_ids().await.unwrap().value, ["1"]);
    client.set_liked("1", "1", false).await.unwrap();
    assert!(client.liked_ids().await.unwrap().value.is_empty());
    assert!(matches!(
        client.set_liked("0", "1", true).await,
        Err(Error::StaleResponse)
    ));
    let calls = backend.requests.lock().unwrap();
    assert_eq!(calls.iter().filter(|v| v.0 == "/api/song/like").count(), 2);
}

#[tokio::test]
async fn old_account_results_and_expired_identity_cannot_cross_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, session) = client(dir.path(), backend.clone());
    client.restore_account().await.unwrap();
    let client = Arc::new(client);
    backend.hold.store(true, Ordering::SeqCst);
    let future = {
        let client = client.clone();
        tokio::spawn(async move { client.search_songs("test", PageRequest::default()).await })
    };
    backend.started.notified().await;
    session.detach().unwrap();
    backend.release.notify_one();
    assert!(matches!(future.await.unwrap(), Err(Error::StaleResponse)));
    backend.expired.store(true, Ordering::SeqCst);
    let expired = client.restore_account().await.unwrap();
    assert!(expired.nickname.is_none());
    assert!(matches!(
        expired.status,
        cumulus_core::account::SessionStatus::Expired
    ));
    let failure = ClientError::from(Error::service(
        "SECRET",
        json!(301),
        json!({"cookie":"DO_NOT_EXPORT"}),
    ));
    let output = serde_json::to_string(&failure).unwrap();
    assert!(!output.contains("SECRET"));
    assert!(!output.contains("DO_NOT_EXPORT"));
    assert_eq!(failure.code, "session_expired");
}
