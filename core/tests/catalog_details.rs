use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::{
        CatalogApi,
        details::{CatalogDetails, CatalogQuery},
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
    calls: Mutex<Vec<(String, Value)>>,
    fail: AtomicBool,
    wrong: AtomicBool,
    omit_dynamic: AtomicBool,
    expired_rights: AtomicBool,
    extra_privileges: AtomicBool,
    stalled_video: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
fn song(id: &str) -> Value {
    json!({"id":id,"name":"测试歌曲","ar":[{"id":12,"name":"测试歌手"}],"al":{"id":7,"name":"测试专辑"},"privilege":{"id":id,"pl":999},"cookie":"PRIVATE"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::Response("fixture read failed"));
            }
            Ok(match path {
                "/api/artist/v3/detail" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"artist":{"id":if self.wrong.load(Ordering::SeqCst){13}else{12},"name":"测试歌手","musicSize":50},"hotSongs":[song("1"),song("2"),song("3")],"more":false})
                }
                "/api/song/enhance/privilege" => {
                    if self.extra_privileges.load(Ordering::SeqCst) {
                        return Ok(
                            json!({"code":200,"data":[{"id":2,"pl":20},{"id":1,"pl":10},{"id":8,"pl":8},{"id":7,"pl":7},{"id":8,"pl":12}]}),
                        );
                    }
                    json!({"code":200,"data":[{"id":2,"pl":if self.expired_rights.load(Ordering::SeqCst){0}else{320000}},{"id":1,"pl":128000}]})
                }
                "/api/artist/detail/dynamic" => {
                    if self.omit_dynamic.load(Ordering::SeqCst) {
                        json!({"code":200})
                    } else {
                        json!({"code":200,"followed":true,"concert":{"onlineCount":1}})
                    }
                }
                "/api/v2/artist/songs" => {
                    json!({"code":200,"songs":[song(&(100+payload["offset"].as_u64().unwrap()).to_string())],"total":50,"more":true})
                }
                "/api/artist/albums/12" => {
                    json!({"code":200,"artist":{"id":12,"albumSize":1},"hotAlbums":if payload["offset"]==0{vec![json!({"albumId":7,"name":"专辑"})]}else{vec![]},"more":false})
                }
                "/api/artist/introduction" => {
                    json!({"code":200,"introduction":[{"ti":"简介","txt":"第一行\n第二行"}]})
                }
                "/api/discovery/simiArtist" => {
                    json!({"code":200,"artists":[{"id":13,"name":"相似歌手"}],"more":false})
                }
                "/api/mlog/artist/video" => {
                    if self.stalled_video.load(Ordering::SeqCst) {
                        return Ok(
                            json!({"code":200,"data":{"records":[],"page":{"size":20,"cursor":"0","more":true}}}),
                        );
                    }
                    json!({"code":200,"data":{"records":[{"id":"opaque-video-id","resource":{"mlogBaseData":{"type":2}}}],"page":{"size":20,"cursor":"900719925474099312345","more":false}}})
                }
                "/api/concert/list/byartist" => {
                    json!({"code":200,"simpleConcerts":[{"id":15,"name":"演出"}],"total":1})
                }
                "/api/album/detail/dynamic" => json!({"code":200,"isSub":true}),
                "/api/album/privilege" => {
                    json!({"code":200,"data":[{"id":2,"pl":320000},{"id":1,"pl":128000}]})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"common"}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"albumId":7,"name":"专辑"},"songs":[song("2"),song("1")]})
                }
                _ => panic!("unexpected {path}"),
            })
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    CatalogDetails,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let service = CatalogDetails::new(session.clone()).unwrap();
    (dir, api, session, service)
}

#[tokio::test]
async fn top_songs_merge_privileges_by_id_preserve_unknowns_and_always_revalidate() {
    let (_dir, api, session, service) = fixture();
    let query = CatalogQuery::Artist { id: "12".into() };
    let token = session.token().unwrap();
    let first = service.page_for(&token, &query).await.unwrap();
    let songs = first.songs().unwrap();
    assert_eq!(songs[0]["privilege"]["pl"], 128000);
    assert_eq!(songs[1]["privilege"]["pl"], 320000);
    assert!(songs[2].get("privilege").is_none());
    assert!(api.calls.lock().unwrap()[1].1.get("trialMode").is_none());
    api.expired_rights.store(true, Ordering::SeqCst);
    let second = service.page_for(&token, &query).await.unwrap();
    assert_eq!(second.songs().unwrap()[1]["privilege"]["pl"], 0);
    assert_eq!(
        first.revision, second.revision,
        "queue revision follows order, not temporary privileges"
    );
    assert_eq!(api.calls.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn query_pages_and_dynamic_state_are_distinct_persistent_snapshots() {
    let (dir, api, session, service) = fixture();
    let token = session.token().unwrap();
    let queries = [
        CatalogQuery::ArtistDynamic { id: "12".into() },
        CatalogQuery::ArtistSongs {
            id: "12".into(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistSongs {
            id: "12".into(),
            offset: 20,
            limit: 20,
        },
        CatalogQuery::ArtistAlbums {
            id: "12".into(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistIntroduction { id: "12".into() },
        CatalogQuery::SimilarArtists {
            id: "12".into(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistVideos {
            id: "12".into(),
            cursor: "0".into(),
            size: 20,
        },
        CatalogQuery::ArtistConcerts {
            id: "12".into(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::Album { id: "7".into() },
    ];
    let mut snapshots = vec![];
    for query in &queries {
        snapshots.push(service.page_for(&token, query).await.unwrap());
    }
    assert_ne!(snapshots[1].revision, snapshots[2].revision);
    assert_eq!(snapshots[1].songs().unwrap()[0]["id"], "100");
    assert_eq!(snapshots[2].songs().unwrap()[0]["id"], "120");
    assert_eq!(snapshots[8].songs().unwrap()[0]["id"], "2");
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    reopened
        .adopt(api.clone(), json!({"userId":9}), &reopened.token().unwrap())
        .unwrap();
    let restored = CatalogDetails::new(reopened.clone()).unwrap();
    for saved in snapshots {
        let actual = restored
            .cached_for(&reopened.token().unwrap(), &saved.query)
            .unwrap()
            .unwrap();
        assert_eq!(actual.response, saved.response);
        assert_eq!(actual.revision, saved.revision);
        assert_eq!(actual.fetched_at, saved.fetched_at);
    }
    reopened
        .adopt(api, json!({"userId":10}), &reopened.token().unwrap())
        .unwrap();
    for query in &queries {
        assert!(
            restored
                .cached_for(&reopened.token().unwrap(), query)
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn failed_or_mismatched_refresh_does_not_replace_verified_snapshots() {
    let (_dir, api, session, service) = fixture();
    let token = session.token().unwrap();
    let query = CatalogQuery::Artist { id: "12".into() };
    let saved = service.page_for(&token, &query).await.unwrap();
    api.fail.store(true, Ordering::SeqCst);
    assert!(service.page_for(&token, &query).await.is_err());
    api.fail.store(false, Ordering::SeqCst);
    api.wrong.store(true, Ordering::SeqCst);
    assert!(service.page_for(&token, &query).await.is_err());
    assert_eq!(
        service
            .cached_for(&token, &query)
            .unwrap()
            .unwrap()
            .response,
        saved.response
    );
    let dynamic = CatalogQuery::ArtistDynamic { id: "12".into() };
    let saved = service.page_for(&token, &dynamic).await.unwrap();
    api.omit_dynamic.store(true, Ordering::SeqCst);
    assert!(service.page_for(&token, &dynamic).await.is_err());
    assert_eq!(
        service
            .cached_for(&token, &dynamic)
            .unwrap()
            .unwrap()
            .response,
        saved.response
    );
}

#[tokio::test]
async fn account_change_rejects_late_results_and_duplicate_refresh_is_bounded() {
    let (_dir, api, session, service) = fixture();
    let service = Arc::new(service);
    let token = session.token().unwrap();
    let query = CatalogQuery::Artist { id: "12".into() };
    api.hold.store(true, Ordering::SeqCst);
    let task = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        let query = query.clone();
        async move { service.page_for(&token, &query).await }
    });
    api.started.notified().await;
    assert!(matches!(
        service.page_for(&token, &query).await,
        Err(Error::MutationBusy)
    ));
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        service
            .cached_for(&session.token().unwrap(), &query)
            .unwrap()
            .is_none()
    );
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(
        service
            .cached_for(&session.token().unwrap(), &query)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn privilege_scene_is_optional_and_invalid_input_never_sends_a_request() {
    let (_dir, api, _session, _service) = fixture();
    let catalog = CatalogApi::new(api.clone());
    assert!(catalog.privileges(&["1".into()], Some(0)).await.is_err());
    assert!(catalog.privileges(&["bad".into()], None).await.is_err());
    assert!(api.calls.lock().unwrap().is_empty());
    catalog
        .privileges(&["1".into(), "2".into(), "1".into()], Some(-1))
        .await
        .unwrap();
    {
        let calls = api.calls.lock().unwrap();
        assert_eq!(calls[0].1, json!({"ids":r#"["1","2","1"]"#,"trialMode":-1}));
    }
    catalog.privileges(&[], None).await.unwrap();
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1,
        json!({"ids":"[]"})
    );
}

#[tokio::test]
async fn official_privilege_merge_appends_missing_metadata_in_reply_order_and_keeps_last_value() {
    let (_dir, api, session, service) = fixture();
    api.extra_privileges.store(true, Ordering::SeqCst);
    let saved = service
        .page_for(
            &session.token().unwrap(),
            &CatalogQuery::Artist { id: "12".into() },
        )
        .await
        .unwrap();
    let songs = saved.songs().unwrap();
    assert_eq!(
        songs
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "8", "7", "8"]
    );
    assert_eq!(songs[3]["privilege"]["pl"], 12);
    assert_eq!(songs[4]["privilege"]["pl"], 7);
    assert!(songs[3].get("name").is_none());
    assert_eq!(songs[3]["artists"], json!([]));
}

#[tokio::test]
async fn stalled_video_pagination_does_not_replace_the_last_known_page() {
    let (_dir, api, session, service) = fixture();
    let token = session.token().unwrap();
    let query = CatalogQuery::ArtistVideos {
        id: "12".into(),
        cursor: "000".into(),
        size: 20,
    };
    let saved = service.page_for(&token, &query).await.unwrap();
    api.stalled_video.store(true, Ordering::SeqCst);
    assert!(service.page_for(&token, &query).await.is_err());
    assert_eq!(
        service
            .cached_for(&token, &query)
            .unwrap()
            .unwrap()
            .response,
        saved.response
    );
}
