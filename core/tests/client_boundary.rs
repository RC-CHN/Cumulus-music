use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        ClientError, MusicClient,
        dto::{PageRequest, PlaylistCollectSort, ProfileRelationKind},
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
    json!({"id":"7","userId":"9","name":"喜欢的音乐","specialType":5,"privacy":10,"creator":{"nickname":"用户"},"trackCount":999,"subscribedCount":25,"coverImgUrl":"http://p2.music.126.net/a.jpg","trackIds":[{"id":"1"},{"id":"1234abcd0000efab1234abcd0000efab","v":-1},{"id":"3"},{"id":"4"}]})
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
                "/api/w/v1/user/detail/81" => {
                    json!({"code":200,"profile":{"userId":81,"nickname":"公开作者","signature":"签名","follows":21,"followeds":2,"token":"DO_NOT_EXPORT","avatarUrl":"http://p1.music.126.net/user.png?secret=DO_NOT_EXPORT"}})
                }
                "/api/w/v1/user/detail/82" => json!({"code":404}),
                "/api/w/v1/user/detail/83" => {
                    json!({"code":200,"profile":{"userId":81,"nickname":"不匹配用户"}})
                }
                "/api/user/personal/page/info" => match payload["userId"].as_str() {
                    Some("81") => {
                        json!({"code":200,"data":{"userRelationDTO":{"canSeeFansOrFollows":true}}})
                    }
                    Some("82") => {
                        json!({"code":200,"data":{"userRelationDTO":{"canSeeFansOrFollows":false,"fansOrFollowsToastText":"合成隐私拒绝"}}})
                    }
                    Some("83") => json!({"code":200,"data":{}}),
                    _ => panic!("unexpected relationship privacy lookup"),
                },
                "/api/user/getfollows/81" => match payload["offset"].as_u64() {
                    Some(0) => {
                        json!({"code":200,"follow":[{"userId":"9007199254740993","nickname":"安全用户","remarkName":"测试备注","mutual":true,"followed":true,"avatarUrl":"http://p1.music.126.net/user.png?secret=DO_NOT_EXPORT","token":"DO_NOT_EXPORT"},{"userId":"bad/path","nickname":"DO_NOT_EXPORT"},{"userId":9,"nickname":"自己","followed":true,"mutual":true}],"size":21,"more":true})
                    }
                    Some(20) => {
                        json!({"code":200,"follow":[{"userId":35,"nickname":"续页用户"}],"size":21,"more":false})
                    }
                    _ => panic!("unexpected relationship offset"),
                },
                "/api/user/getfolloweds/" => {
                    json!({"code":200,"followeds":[],"size":0,"more":false})
                }
                "/api/user/follow/users/mixed/get/v2" => {
                    match (payload["scene"].as_u64(), &payload["page"]["cursor"]) {
                        (Some(0), cursor) if cursor == "0" => {
                            json!({"code":200,"data":{"records":[{"artistInfo":{"id":54,"name":"混合歌手","picUrl":"https://foreign.invalid/private"},"userProfile":{"userId":81,"nickname":"绑定用户","signature":"绑定签名","avatarUrl":"http://p1.music.126.net/art.png?token=DO_NOT_EXPORT"}}],"page":{"more":true,"cursor":9007199254740993_u64}}})
                        }
                        (Some(0), cursor) if cursor == 9007199254740993_u64 => {
                            json!({"code":200,"data":{"records":[{"userProfile":{"userId":81,"nickname":"混合用户"}}],"page":{"more":false}}})
                        }
                        (Some(1), _) => {
                            json!({"code":200,"data":{"records":[{"artistInfo":{"id":55,"name":"歌手分类"}}],"page":{"more":false}}})
                        }
                        (Some(2), _) => {
                            json!({"code":200,"data":{"records":[{"userProfile":{"userId":83,"nickname":"用户分类"}}],"page":{"more":false}}})
                        }
                        _ => panic!("unexpected own relationship route"),
                    }
                }
                "/api/user/playlist" if payload["uid"] == "81" => {
                    if payload["offset"] == 0 {
                        json!({"code":200,"more":true,"playlist":[
                            {"id":901,"userId":81,"name":"公开创建","privacy":0},
                            {"id":903,"userId":81,"name":"DO_NOT_EXPORT","privacy":10}]})
                    } else {
                        assert_eq!(payload["offset"], 1000);
                        json!({"code":200,"more":false,"playlist":[
                            {"id":902,"userId":80,"name":"公开收藏","privacy":0},
                            {"id":904,"userId":81,"name":"DO_NOT_EXPORT"}]})
                    }
                }
                "/api/user/playlist" => json!({"code":200,"playlist":[playlist()],"more":false}),
                "/api/user/playlist/v2" => match payload["offset"].as_u64() {
                    Some(0) => json!({"code":200,"more":true,"playlist":[
                        playlist(),
                        {"id":11,"userId":8,"name":"别人的歌单"},
                        {"id":13,"userId":9,"name":"常用 13","cookie":"DO_NOT_EXPORT"},
                        {"id":12,"userId":9,"name":"常用 12"}
                    ]}),
                    Some(1) => {
                        json!({"code":200,"playlist":[{"id":14,"userId":8,"name":"仅收藏"}]})
                    }
                    _ => json!({"code":200,"playlist":[],"more":true}),
                },
                "/api/user/playlist/getTop" if payload["userId"] == "81" => {
                    json!({"code":200,"data":{"topItemList":[]}})
                }
                "/api/djradio/get/create/sub/count" if payload == json!({"userId":"81"}) => {
                    json!({"code":200,"createRadioCount":2,"subRadioCount":1})
                }
                "/api/v6/playlist/detail" => {
                    let mut value = playlist();
                    value["id"] = payload["id"].clone();
                    value["creator"]["avatarUrl"] =
                        json!("http://p1.music.126.net/author.jpg?token=DO_NOT_EXPORT");
                    value["privacy"] = json!(0);
                    value["playCount"] = json!(12600);
                    value["createTime"] = json!(1705320000000_u64);
                    value["updateTime"] = json!(1707998400000_u64);
                    match payload["id"].as_str() {
                        Some("8") => {
                            value["privacy"] = json!(10);
                            value["specialType"] = json!(10);
                            value["creator"]["avatarUrl"] =
                                json!("https://foreign.invalid/private");
                        }
                        Some("9") => {
                            value["playCount"] = json!(9_007_199_254_740_992_u64);
                            value["specialType"] = json!(10);
                            value["updateTime"] = json!(0);
                        }
                        _ => {}
                    }
                    json!({"code":200,"playlist":value})
                }
                "/api/playlist/category/list" => json!({"code":200,
                    "playlists":[{"id":1,"name":"分类歌单","coverImgUrl":"http://p1.music.126.net/a.jpg?secret=DO_NOT_EXPORT"}],
                    "playlistIds":(1..=26).collect::<Vec<_>>() }),
                "/api/playlist/list/get" => json!({"code":200,
                    "playlists":[{"id":26,"name":"第二页乙"},{"id":25,"name":"第二页甲"}]}),
                "/api/playlist/subscribers" => match payload["offset"].as_u64() {
                    Some(0) => {
                        json!({"code":200,"subscribers":[{"userId":81,"nickname":"收藏者甲","signature":"签名","avatarUrl":"http://p1.music.126.net/a.jpg?secret=DO_NOT_EXPORT","token":"DO_NOT_EXPORT"}],"more":true,"total":25})
                    }
                    Some(24) => {
                        json!({"code":200,"subscribers":[{"userId":"bad/path","nickName":"收藏者乙","avatarUrl":"https://foreign.invalid/private"}],"more":false,"total":25})
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
        ["1", "1234abcd0000efab1234abcd0000efab", "3"]
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
    {
        let requests = backend.requests.lock().unwrap();
        let page_request = requests
            .iter()
            .find(|(path, _)| path == "/api/v3/song/detail")
            .unwrap();
        let rows: Vec<Value> = serde_json::from_str(page_request.1["c"].as_str().unwrap()).unwrap();
        assert_eq!(
            rows.iter().map(|row| &row["id"]).collect::<Vec<_>>(),
            [&json!("1"), &json!("3")]
        );
    }
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
        Error::service(endpoint, json!(403), json!({})),
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

#[test]
fn only_transient_json_service_codes_from_the_source_endpoint_allow_source_retry() {
    let endpoint = "/api/song/enhance/player/url/v1";
    for code in [json!(429), json!(500), json!("503"), json!(599)] {
        let failure = ClientError::from(Error::service(
            endpoint,
            code,
            json!({"url":"https://secret.invalid/signed","md5":"DO_NOT_EXPORT"}),
        ));
        assert_eq!(failure.code, "playback_source_temporary");
        assert!(failure.retryable);
        let wire = serde_json::to_string(&failure).unwrap();
        assert!(!wire.contains(endpoint));
        assert!(!wire.contains("secret.invalid"));
        assert!(!wire.contains("DO_NOT_EXPORT"));
    }
    let unrelated = ClientError::from(Error::service("/api/unrelated", json!(503), json!({})));
    assert_eq!(unrelated.code, "service_rejected");
    assert!(!unrelated.retryable);
}

#[test]
fn a_missing_libmpv_sdk_uses_the_installation_diagnostic() {
    let failure = ClientError::from(Error::Playback {
        operation: "library unavailable",
        code: -1,
    });
    assert_eq!(failure.code, "audio_unavailable");
    assert!(failure.retryable);
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
async fn playlist_collection_sorts_on_the_service_and_filters_editable_targets() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    let recent = PlaylistCollectSort::Recently;
    assert!(matches!(
        client.playlist_collect_page("0", recent, 0).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.playlist_collect_page(&epoch, recent, 1000).await,
        Err(Error::InvalidInput(_))
    ));
    let first = client
        .playlist_collect_page(&epoch, recent, 0)
        .await
        .unwrap();
    assert_eq!(first.epoch, epoch);
    assert_eq!(first.value.next_page, Some(1));
    assert_eq!(
        first
            .value
            .playlists
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["13", "12"]
    );
    assert!(
        !serde_json::to_string(&first.value)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
    let last = client
        .playlist_collect_page(&epoch, recent, 1)
        .await
        .unwrap();
    assert!(last.value.playlists.is_empty());
    assert_eq!(last.value.next_page, None);
    assert!(matches!(
        client
            .playlist_collect_page(&epoch, PlaylistCollectSort::Default, 2)
            .await,
        Err(Error::Response(_))
    ));
    let requests = backend.requests.lock().unwrap();
    let payloads: Vec<_> = requests
        .iter()
        .filter(|(path, _)| path.ends_with("/playlist/v2"))
        .map(|(_, payload)| payload.clone())
        .collect();
    assert_eq!(
        payloads,
        [
            json!({"uid":"9","offset":0,"limit":10,"sortType":"recently"}),
            json!({"uid":"9","offset":1,"limit":10,"sortType":"recently"}),
            json!({"uid":"9","offset":2,"limit":10,"sortType":"default"}),
        ]
    );
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
            "userId": "81",
            "nickname": "收藏者甲",
            "signature": "签名",
            "avatarUrl": "https://p1.music.126.net/a.jpg?param=320y320",
        })
    );
    let second = client.playlist_subscribers(&epoch, "7", 24).await.unwrap();
    assert_eq!(second.value.next_offset, None);
    assert_eq!(second.value.previous_offset, Some(0));
    assert_eq!(second.value.subscribers[0].avatar_url, None);
    assert_eq!(second.value.subscribers[0].user_id, None);
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
async fn playlist_header_projects_official_date_public_creator_artwork_and_public_play_count() {
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
    assert_eq!(ordinary["playCount"], 12600);
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
    assert_eq!(updated["private"], true);
    assert!(updated["playCount"].is_null());
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
    assert!(missing["playCount"].is_null());
}

#[tokio::test]
async fn playlist_tag_browse_uses_official_first_and_continuation_reads() {
    use cumulus_core::client::dto::PlaylistTagQuery;
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    assert!(
        client
            .playlist_tag_page(
                "0",
                PlaylistTagQuery::Category {
                    tag: "轻音乐".into()
                }
            )
            .await
            .is_err()
    );
    let first = client
        .playlist_tag_page(
            &epoch,
            PlaylistTagQuery::Category {
                tag: "轻音乐".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(first.epoch, epoch);
    assert_eq!(first.value.remaining_ids, ["25", "26"]);
    assert_eq!(
        first.value.items[0].cover_url.as_deref(),
        Some("https://p1.music.126.net/a.jpg?param=320y320")
    );
    let second = client
        .playlist_tag_page(
            &epoch,
            PlaylistTagQuery::Items {
                ids: first.value.remaining_ids,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second
            .value
            .items
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["26", "25"]
    );
    assert!(
        client
            .playlist_tag_page(
                &epoch,
                PlaylistTagQuery::Items {
                    ids: vec!["25".into()]
                }
            )
            .await
            .is_err()
    );
    let calls = backend.requests.lock().unwrap();
    assert!(calls.contains(&(
        "/api/playlist/category/list".into(),
        json!({"cat":"轻音乐","limit":24,"newStyle":true})
    )));
    assert!(calls.contains(&(
        "/api/playlist/list/get".into(),
        json!({"ids":"[25,26]","newStyle":true})
    )));
}

#[tokio::test]
async fn playlist_creator_profile_checks_identity_and_only_projects_public_playlists() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    assert!(client.playlist_creator_profile("0", "81").await.is_err());
    assert!(
        client
            .playlist_creator_profile(&epoch, "bad/id")
            .await
            .is_err()
    );
    let value = client.playlist_creator_profile(&epoch, "81").await.unwrap();
    assert_eq!(value.epoch, epoch);
    let view = value.value.unwrap();
    assert_eq!(view.id, "81");
    assert_eq!(view.nickname, "公开作者");
    assert_eq!(view.following_count, Some(21));
    assert_eq!(view.followers_count, Some(2));
    assert_eq!(
        view.avatar_url.as_deref(),
        Some("https://p1.music.126.net/user.png?param=320y320")
    );
    assert_eq!(
        view.created
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["901"]
    );
    assert_eq!(
        view.collected
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["902"]
    );
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
    assert!(
        client
            .playlist_creator_profile(&epoch, "82")
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert!(client.playlist_creator_profile(&epoch, "83").await.is_err());
    let calls = backend.requests.lock().unwrap();
    assert!(calls.contains(&(
        "/api/w/v1/user/detail/81".into(),
        json!({"all":true,"userId":"81"})
    )));
    let list_reads: Vec<_> = calls
        .iter()
        .filter(|(path, _)| path == "/api/user/playlist")
        .collect();
    assert_eq!(list_reads.len(), 2);
    assert_eq!(list_reads[0].1, json!({"uid":"81","offset":0,"limit":1000}));
    assert_eq!(
        list_reads[1].1,
        json!({"uid":"81","offset":1000,"limit":1000})
    );
}

#[tokio::test]
async fn profile_relation_routes_check_guest_privacy_and_keep_native_cursor_scoped() {
    use ProfileRelationKind as Kind;
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(Backend::default());
    let (client, _) = client(dir.path(), backend.clone());
    let epoch = client.restore_account().await.unwrap().epoch;
    backend.requests.lock().unwrap().clear();

    assert!(
        client
            .profile_relation_page("0", "81", Kind::Following, None)
            .await
            .is_err()
    );
    assert!(
        client
            .profile_relation_access(&epoch, "bad/path")
            .await
            .is_err()
    );
    let denied = client.profile_relation_access(&epoch, "82").await.unwrap();
    assert_eq!(
        json!(denied.value),
        json!({"state":"denied","message":"合成隐私拒绝"})
    );
    let denied = client
        .profile_relation_page(&epoch, "82", Kind::Followers, None)
        .await
        .unwrap();
    assert_eq!(json!(denied.value)["state"], "denied");
    assert!(
        client
            .profile_relation_page(&epoch, "83", Kind::Following, None)
            .await
            .is_err()
    );
    assert!(
        client
            .profile_relation_page(&epoch, "81", Kind::FollowingArtists, None)
            .await
            .is_err()
    );
    let allowed = client.profile_relation_access(&epoch, "81").await.unwrap();
    assert_eq!(
        json!(allowed.value),
        json!({"state":"allowed","isSelf":false})
    );
    let first = json!(
        client
            .profile_relation_page(&epoch, "81", Kind::Following, None)
            .await
            .unwrap()
            .value
    );
    assert_eq!(first["recordCount"], 21);
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["items"][1]["id"], "9");
    assert!(first["items"][1]["relationship"].is_null());
    assert_eq!(
        first["items"][0],
        json!({
            "id":"9007199254740993","kind":"user","name":"安全用户","remark":"测试备注",
            "avatarUrl":"https://p1.music.126.net/user.png?param=320y320","signature":null,
            "relationship":"mutual"
        })
    );
    let next = first["continuation"].as_str().unwrap();
    assert!(
        client
            .profile_relation_page(&epoch, "81", Kind::Followers, Some(next))
            .await
            .is_err()
    );
    assert!(
        client
            .profile_relation_page(&epoch, "82", Kind::Following, Some(next))
            .await
            .is_err()
    );
    let second = json!(
        client
            .profile_relation_page(&epoch, "81", Kind::Following, Some(next))
            .await
            .unwrap()
            .value
    );
    assert_eq!(second["items"][0]["id"], "35");
    assert!(second["continuation"].is_null());
    client
        .profile_relation_page(&epoch, "81", Kind::Followers, None)
        .await
        .unwrap();
    let own = client.profile_relation_access(&epoch, "9").await.unwrap();
    assert_eq!(json!(own.value), json!({"state":"allowed","isSelf":true}));
    client
        .profile_relation_page(&epoch, "9", Kind::Followers, None)
        .await
        .unwrap();
    let mixed = json!(
        client
            .profile_relation_page(&epoch, "9", Kind::Following, None)
            .await
            .unwrap()
            .value
    );
    assert_eq!(mixed["items"][0]["kind"], "artist");
    assert_eq!(mixed["items"][0]["id"], "54");
    assert_eq!(mixed["items"][0]["signature"], "绑定签名");
    assert!(!mixed.to_string().contains("DO_NOT_EXPORT"));
    assert!(!mixed.to_string().contains("9007199254740993"));
    let mixed_next = mixed["continuation"].as_str().unwrap();
    let continued = json!(
        client
            .profile_relation_page(&epoch, "9", Kind::Following, Some(mixed_next))
            .await
            .unwrap()
            .value
    );
    assert_eq!(continued["items"][0]["kind"], "user");
    client
        .profile_relation_page(&epoch, "9", Kind::FollowingArtists, None)
        .await
        .unwrap();
    client
        .profile_relation_page(&epoch, "9", Kind::FollowingUsers, None)
        .await
        .unwrap();
    let calls = backend.requests.lock().unwrap();
    let privacy = |id: &str| ("/api/user/personal/page/info".into(), json!({"userId":id}));
    let follow = |offset| {
        (
            "/api/user/getfollows/81".into(),
            json!({"offset":offset,"limit":20,"getcounts":true}),
        )
    };
    let fans = |id: &str| {
        (
            "/api/user/getfolloweds/".into(),
            json!({"userId":id,"time":0,"offset":0,"limit":20,"getcounts":true}),
        )
    };
    let mixed = |scene, cursor: Value| {
        (
            "/api/user/follow/users/mixed/get/v2".into(),
            json!({"page":{"size":19,"cursor":cursor},"size":19,"scene":scene,"sortType":0,"authority":false}),
        )
    };
    assert_eq!(
        *calls,
        [
            privacy("82"),
            privacy("82"),
            privacy("83"),
            privacy("81"),
            privacy("81"),
            follow(0),
            follow(20),
            privacy("81"),
            fans("81"),
            fans("9"),
            mixed(0, json!("0")),
            mixed(0, json!(9007199254740993_u64)),
            mixed(1, json!("0")),
            mixed(2, json!("0")),
        ]
    );
}
