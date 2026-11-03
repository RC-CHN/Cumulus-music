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
    json!({"id":"7","userId":"9","name":"喜欢的音乐","specialType":5,"privacy":10,"creator":{"nickname":"用户"},"trackCount":999,"subscribedCount":25,"coverImgUrl":"http://p2.music.126.net/a.jpg","trackIds":[{"id":"1"},{"id":"2","v":-1},{"id":"3"},{"id":"4"}]})
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
                "/api/v6/playlist/detail" => {
                    let mut value = playlist();
                    value["id"] = payload["id"].clone();
                    value["creator"]["avatarUrl"] =
                        json!("http://p1.music.126.net/author.jpg?token=DO_NOT_EXPORT");
                    value["createTime"] = json!(1705320000000_u64);
                    value["updateTime"] = json!(1707998400000_u64);
                    match payload["id"].as_str() {
                        Some("8") => {
                            value["specialType"] = json!(10);
                            value["creator"]["avatarUrl"] =
                                json!("https://foreign.invalid/private");
                        }
                        Some("9") => {
                            value["specialType"] = json!(10);
                            value["updateTime"] = json!(0);
                        }
                        _ => {}
                    }
                    json!({"code":200,"playlist":value})
                }
                "/api/playlist/subscribers" => match payload["offset"].as_u64() {
                    Some(0) => {
                        json!({"code":200,"subscribers":[{"userId":81,"nickname":"收藏者甲","signature":"签名","avatarUrl":"http://p1.music.126.net/a.jpg?secret=DO_NOT_EXPORT","token":"DO_NOT_EXPORT"}],"more":true,"total":25})
                    }
                    Some(24) => {
                        json!({"code":200,"subscribers":[{"userId":82,"nickName":"收藏者乙","avatarUrl":"https://foreign.invalid/private"}],"more":false,"total":25})
                    }
                    _ => json!({"code":200,"subscribers":[],"more":true}),
                },
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

#[test]
fn playback_rejection_is_safe_and_not_a_retryable_transport_failure() {
    let endpoint = "/api/song/enhance/player/url/v1";
    for error in [
        Error::service(
            endpoint,
            json!(404),
            json!({"url":"https://secret.invalid/signed","md5":"DO_NOT_EXPORT"}),
        ),
        Error::Response("no playback resource for requested track"),
    ] {
        let failure = ClientError::from(error);
        assert_eq!(failure.code, "playback_source_unavailable");
        assert!(!failure.retryable);
        let wire = serde_json::to_string(&failure).unwrap();
        assert!(!wire.contains(endpoint));
        assert!(!wire.contains("secret.invalid"));
        assert!(!wire.contains("DO_NOT_EXPORT"));
    }
    let expired = ClientError::from(Error::service(endpoint, json!(301), json!({})));
    assert_eq!(expired.code, "session_expired");
    let transport = ClientError::from(Error::Transport("https://secret.invalid/signed".into()));
    assert_eq!(transport.code, "network");
    assert!(transport.retryable);
    assert!(
        !serde_json::to_string(&transport)
            .unwrap()
            .contains("secret.invalid")
    );
}

#[tokio::test]
async fn like_recovery_view_is_bounded_local_and_account_scoped() {
    use cumulus_core::{account::resources::MutationPhase, client::dto::LikeRecoveryCommand};
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, session) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    for id in 1..=51 {
        store.put("9","resource-mutations",&format!("liked:{id}"),&json!({"kind":"liked","key":id.to_string(),"before":false,"desired":true,"created_at":1.0,"phase":"uncertain","error_class":"DO_NOT_EXPORT"})).unwrap();
    }
    store
        .put(
            "9",
            "resource-mutations",
            "settings:shareSetting",
            &json!({"synthetic":"damaged unrelated settings"}),
        )
        .unwrap();
    let page = client.like_recovery(&epoch, 0).unwrap();
    assert_eq!(page.value.total, 51);
    assert_eq!(page.value.operations.len(), 20);
    assert!(!page.value.operations[0].can_accept);
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
    let tail = client.like_recovery(&epoch, usize::MAX).unwrap();
    assert_eq!(tail.value.offset, 40);
    assert_eq!(tail.value.operations.len(), 11);
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
    assert!(matches!(
        client.like_recovery("old", 0),
        Err(Error::StaleResponse)
    ));
    let target = page
        .value
        .operations
        .into_iter()
        .find(|op| op.id == "1")
        .unwrap();
    let checked = client
        .recover_like(
            &epoch,
            LikeRecoveryCommand::Reconcile {
                id: target.id.clone(),
                revision: target.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(checked.value.observed, Some(false));
    assert!(checked.value.can_accept);
    let accepted = client
        .recover_like(
            &epoch,
            LikeRecoveryCommand::AcceptCurrent {
                id: "1".into(),
                revision: checked.value.revision,
                observed: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(accepted.value.phase, MutationPhase::AcceptedCurrentState);
    assert_eq!(client.like_recovery(&epoch, 0).unwrap().value.total, 50);
    assert!(
        !backend
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.0 == "/api/song/like")
    );
    session.detach().unwrap();
    assert!(client.like_recovery(&epoch, 0).is_err());
}

#[tokio::test]
async fn playlist_subscribers_are_epoch_bound_paged_and_safe_to_render() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    assert_eq!(
        client.playlists().await.unwrap().value[0].subscribed_count,
        Some(25)
    );
    assert!(matches!(
        client.playlist_subscribers("0", "7", 0).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.playlist_subscribers(&epoch, "7", 1).await,
        Err(Error::InvalidInput(_))
    ));
    let first = client.playlist_subscribers(&epoch, "7", 0).await.unwrap();
    assert_eq!(first.epoch, epoch);
    assert_eq!(first.value.next_offset, Some(24));
    assert_eq!(first.value.previous_offset, None);
    assert_eq!(
        serde_json::to_value(&first.value.subscribers[0]).unwrap(),
        json!({
            "nickname": "收藏者甲",
            "signature": "签名",
            "avatarUrl": "https://p1.music.126.net/a.jpg?param=320y320",
        })
    );
    let second = client.playlist_subscribers(&epoch, "7", 24).await.unwrap();
    assert_eq!(second.value.next_offset, None);
    assert_eq!(second.value.previous_offset, Some(0));
    assert_eq!(second.value.subscribers[0].avatar_url, None);
    assert_eq!(second.value.subscribers[0].nickname, "收藏者乙");
    assert!(matches!(
        client.playlist_subscribers(&epoch, "7", 48).await,
        Err(Error::Response(_))
    ));
    let requests = backend.requests.lock().unwrap();
    let payloads: Vec<_> = requests
        .iter()
        .filter(|(path, _)| path == "/api/playlist/subscribers")
        .map(|(_, payload)| payload.clone())
        .collect();
    assert_eq!(
        payloads,
        [0, 24, 48].map(|offset| json!({"id":"7","limit":24,"offset":offset}))
    );
}

#[tokio::test]
async fn playlist_header_projects_only_the_official_date_and_public_creator_artwork() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend);
    client.restore_account().await.unwrap();
    let header = |value: Option<cumulus_core::client::dto::PlaylistView>| {
        serde_json::to_value(value.unwrap()).unwrap()
    };
    let ordinary = header(
        client
            .playlist_page("7", PageRequest::default())
            .await
            .unwrap()
            .value
            .playlist,
    );
    assert_eq!(
        ordinary["headerDate"],
        json!({"kind":"created","timestampMs":1705320000000_u64})
    );
    assert_eq!(
        ordinary["creatorAvatarUrl"],
        "https://p1.music.126.net/author.jpg?param=320y320"
    );
    let updated = header(
        client
            .playlist_page("8", PageRequest::default())
            .await
            .unwrap()
            .value
            .playlist,
    );
    assert_eq!(
        updated["headerDate"],
        json!({"kind":"updated","timestampMs":1707998400000_u64})
    );
    assert!(updated["creatorAvatarUrl"].is_null());
    let missing = header(
        client
            .playlist_page("9", PageRequest::default())
            .await
            .unwrap()
            .value
            .playlist,
    );
    assert!(
        missing["headerDate"].is_null(),
        "special playlists never fall back to createTime"
    );
}
