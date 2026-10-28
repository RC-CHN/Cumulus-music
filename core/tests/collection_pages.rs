use cumulus_core::{
    Error,
    account::{
        AccountSession,
        collections::{CollectionApi, CollectionQuery, Collections},
        profiles::ProfileRegistry,
    },
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

type Call = (String, Value, bool);
struct Backend {
    responses: Mutex<BTreeMap<String, Value>>,
    calls: Mutex<Vec<Call>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload, cache));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            self.responses
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(Error::Response("unexpected fixture request"))
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let album = json!({"albumId":"90071992547409931234","albumName":"合成专辑","artists":[{"id":12,"name":"歌手"}],"picUrl":"http://p1.music.126.net/test.jpg?token=PRIVATE","size":11,"paidToken":"PRIVATE"});
    let artist = json!({"id":12,"name":"合成歌手","accountId":99,"img1v1Url":"https://p2.music.126.net/test.jpg?token=PRIVATE"});
    let user = json!({"userId":99,"nickname":"合成用户","signature":"简介","avatarUrl":"https://example.invalid/PRIVATE","accountStatus":30,"cookie":"PRIVATE"});
    let api = Arc::new(Backend {
        responses: Mutex::new([
            ("/api/album/sublist", json!({"code":200,"data":[album.clone()],"count":3,"hasMore":true})),
            ("/api/digitalAlbum/purchased", json!({"code":200,"paidAlbums":[album],"total":1})),
            ("/api/user/follow/users/mixed/get/v2", json!({"code":200,"data":{"records":[{"artistInfo":artist,"userProfile":user,"followed":false},{"artistInfo":{"id":13,"name":"歌手2"}},{"userProfile":{"userId":100,"nickname":"用户2"}},{}],"page":{"more":true,"cursor":"90071992547409931234","size":19}}})),
        ].into_iter().map(|(k,v)|(k.into(),v)).collect()),
        calls: Mutex::default(), hold:AtomicBool::new(false), started:Notify::new(),release:Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (root, api, session, client)
}
fn saved() -> CollectionQuery {
    CollectionQuery::SavedAlbums {
        offset: 0,
        limit: 50,
    }
}
fn followed() -> CollectionQuery {
    CollectionQuery::FollowedArtists {
        cursor: "0".into(),
        size: 19,
    }
}

#[tokio::test]
async fn official_requests_preserve_separate_collection_and_follow_flows_and_safe_dtos() {
    let (_root, api, _session, client) = fixture();
    let mut views = Vec::new();
    for query in [saved(), CollectionQuery::PurchasedAlbums, followed()] {
        assert!(
            client
                .collection_page("1", query.clone(), false)
                .await
                .unwrap()
                .value
                .is_none()
        );
        let fresh = client
            .collection_page("1", query.clone(), true)
            .await
            .unwrap()
            .value
            .unwrap();
        let cached = client
            .collection_page("1", query, false)
            .await
            .unwrap()
            .value
            .unwrap();
        assert!(cached.cached);
        assert!(!fresh.cached);
        assert_eq!(
            serde_json::to_value(&fresh.page).unwrap(),
            serde_json::to_value(cached.page).unwrap()
        );
        let value = serde_json::to_value(fresh).unwrap();
        assert!(!value.to_string().contains("PRIVATE"));
        views.push(value);
    }
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            (
                "/api/album/sublist".into(),
                json!({"offset":0,"limit":50,"total":true}),
                false
            ),
            (
                "/api/digitalAlbum/purchased".into(),
                json!({"substatus":true}),
                false
            ),
            (
                "/api/user/follow/users/mixed/get/v2".into(),
                json!({"page":{"size":19,"cursor":"0"},"size":19,"scene":1,"sortType":0,"authority":false}),
                false
            ),
        ]
    );
    assert_eq!(views[0]["page"]["albums"][0]["id"], "90071992547409931234");
    assert_eq!(views[0]["next"]["offset"], 50);
    assert!(views[1]["next"].is_null());
    assert_eq!(views[2]["next"]["cursor"], "90071992547409931234");
    let entries = &views[2]["page"]["entries"];
    assert_eq!(entries.as_array().unwrap().len(), 4);
    assert_eq!(entries[0]["artist"]["id"], "12");
    assert_eq!(entries[0]["user"]["id"], "99");
    assert_eq!(entries[0]["user"]["unavailable"], true);
    assert!(entries[1]["user"].is_null() && entries[2]["artist"].is_null());
    assert!(entries[3]["user"].is_null() && entries[3]["artist"].is_null());
}
#[tokio::test]
async fn failed_or_invalid_reads_preserve_last_page_and_reopen_it() {
    let (root, api, session, client) = fixture();
    let original = client
        .collection_page("1", saved(), true)
        .await
        .unwrap()
        .value
        .unwrap();
    for response in [
        json!({"code":503}),
        json!({"code":200,"data":[],"hasMore":true}),
        json!({"code":200,"data":[{"id":"bad"}],"hasMore":false}),
        json!({"code":200,"data":[]}),
    ] {
        api.responses
            .lock()
            .unwrap()
            .insert("/api/album/sublist".into(), response);
        assert!(client.collection_page("1", saved(), true).await.is_err());
    }
    let reopened = MusicClient::with_session(
        session,
        ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap(),
    );
    let cached = reopened
        .collection_page("1", saved(), false)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        serde_json::to_value(original.page).unwrap(),
        serde_json::to_value(cached.page).unwrap()
    );
    assert_eq!(original.fetched_at, cached.fetched_at);
}
#[tokio::test]
async fn followed_cursor_must_advance_and_malformed_records_are_not_cached() {
    let (_root, api, session, client) = fixture();
    for response in [
        json!({"code":200,"data":{"records":[{"artistInfo":{"id":12}}],"page":{"cursor":"0","more":true}}}),
        json!({"code":200,"data":{"records":[],"page":{"cursor":"1","more":true}}}),
        json!({"code":200,"data":{"records":[{"userProfile":{"userId":"bad"}}],"page":{"more":false}}}),
        json!({"code":200,"data":{"records":[],"page":{"cursor":"1"}}}),
    ] {
        api.responses
            .lock()
            .unwrap()
            .insert("/api/user/follow/users/mixed/get/v2".into(), response);
        assert!(client.collection_page("1", followed(), true).await.is_err());
        assert!(
            Collections::new(session.clone())
                .unwrap()
                .cached_for(&session.token().unwrap(), &followed())
                .unwrap()
                .is_none()
        );
    }
    api.responses.lock().unwrap().insert(
        "/api/digitalAlbum/purchased".into(),
        json!({"code":200,"paidAlbums":[],"total":0}),
    );
    let empty = client
        .collection_page("1", CollectionQuery::PurchasedAlbums, true)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(empty.total, Some(0));
    assert!(empty.next.is_none());
}
#[tokio::test]
async fn late_old_account_reads_cannot_publish_to_either_account() {
    let (_root, api, session, client) = fixture();
    api.hold.store(true, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.collection_page("1", saved(), true).await }
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        client
            .collection_page("2", saved(), false)
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        client.collection_page("1", saved(), false).await,
        Err(Error::StaleResponse)
    ));
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(
        client
            .collection_page("3", saved(), false)
            .await
            .unwrap()
            .value
            .is_none()
    );
}
#[tokio::test]
async fn invalid_queries_are_rejected_before_network_and_each_visit_refreshes() {
    let (_root, api, _session, client) = fixture();
    let domain = CollectionApi::new(api.clone());
    for query in [
        CollectionQuery::SavedAlbums {
            offset: usize::MAX,
            limit: 50,
        },
        CollectionQuery::FollowedArtists {
            cursor: "\n".into(),
            size: 19,
        },
        CollectionQuery::FollowedArtists {
            cursor: "0".into(),
            size: 0,
        },
    ] {
        assert!(domain.page(&query).await.is_err());
    }
    assert!(api.calls.lock().unwrap().is_empty());
    client.collection_page("1", saved(), true).await.unwrap();
    client.collection_page("1", saved(), true).await.unwrap();
    assert_eq!(api.calls.lock().unwrap().len(), 2);
}
