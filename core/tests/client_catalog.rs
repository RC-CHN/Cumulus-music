use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::CatalogQuery},
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
    responses: Mutex<std::collections::BTreeMap<String, Value>>,
    calls: Mutex<Vec<String>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
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
    let song = json!({"id":1,"name":"歌曲","ar":[{"id":12,"name":"歌手"}],"al":{"id":7,"name":"专辑","picUrl":"http://p1.music.126.net/a.jpg?token=PRIVATE"},"url":"https://example.invalid/PRIVATE","cookie":"PRIVATE"});
    let artist = json!({"id":12,"name":"歌手","alias":["别名"],"trans":"译名","musicSize":50,"albumSize":2,"img1v1Url":"https://example.invalid/PRIVATE","picUrl":"http://p2.music.126.net/a.jpg?auth=PRIVATE"});
    let album = json!({"id":7,"name":"专辑","size":2,"artists":[{"id":12,"name":"歌手"}],"picUrl":"https://p3.music.126.net/a.jpg?auth=PRIVATE","paidKey":"PRIVATE"});
    let video = |id: &str, kind: u64, is_mv: bool| json!({"id":id,"isMv":is_mv,"resource":{"mlogBaseData":{"type":kind,"text":"视频","coverUrl":"https://p1.music.126.net/a.jpg?token=PRIVATE","duration":1234},"mlogExtVO":{"artists":[{"id":12,"name":"歌手"}],"playCount":9},"shareUrl":"https://example.invalid/PRIVATE","authorization":"PRIVATE"}});
    let responses=[
        ("/api/artist/v3/detail",json!({"code":200,"artist":artist,"hotSongs":[song.clone()],"more":false})),
        ("/api/song/enhance/privilege",json!({"code":200,"data":[{"id":1,"pl":128000},{"id":8,"pl":128000,"token":"PRIVATE"}]})),
        ("/api/artist/detail/dynamic",json!({"code":200,"followed":false,"concert":{"onlineCount":1},"videoNum":3,"cookie":"PRIVATE"})),
        ("/api/v2/artist/songs",json!({"code":200,"songs":[song.clone()],"more":true,"total":50})),
        ("/api/artist/albums/12",json!({"code":200,"artist":{"id":12,"albumSize":2},"hotAlbums":[album.clone()],"more":true})),
        ("/api/artist/introduction",json!({"code":200,"briefDesc":"概述","introduction":[{"ti":"简介","txt":"一\n二","url":"PRIVATE"}]})),
        ("/api/discovery/simiArtist",json!({"code":200,"artists":[artist]})),
        ("/api/mlog/artist/video",json!({"code":200,"data":{"records":[video("20",3,false),video("21",2,false),video("22",99,false),video("23",2,true),video("opaque",3,false)],"page":{"more":false,"cursor":"90071992547409931234","size":20}}})),
        ("/api/concert/list/byartist",json!({"code":200,"total":2,"simpleConcerts":[{"id":30,"title":"演出","cover":"https://p1.music.126.net/a.jpg?ticket=PRIVATE","time":[1234,"2026-10-28"],"simpleAddress":{"address":"场馆"},"price":"88.00 起","url":"https://example.invalid/PRIVATE"}]})),
        ("/api/album/detail/dynamic",json!({"code":200,"isSub":false})),
        ("/api/album/privilege",json!({"code":200,"data":[{"id":1,"pl":128000}]})),
        ("/api/resource-exposure/config",json!({"code":200,"data":{"resourceType":"common"}})),
        ("/api/album/v3/detail",json!({"code":200,"album":album,"songs":[song]})),
    ].into_iter().map(|(path,value)|(path.into(),value)).collect();
    let api = Arc::new(Backend {
        responses: Mutex::new(responses),
        calls: Mutex::default(),
        hold: AtomicBool::new(false),
        started: Notify::new(),
        release: Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (root, api, session, client)
}
fn queries() -> Vec<CatalogQuery> {
    vec![
        CatalogQuery::Artist { id: "12".into() },
        CatalogQuery::ArtistDynamic { id: "12".into() },
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
            limit: 1,
        },
        CatalogQuery::Album { id: "7".into() },
    ]
}
#[tokio::test]
async fn all_catalog_page_dtos_redact_native_fields_and_preserve_semantics() {
    let (_root, _api, session, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let mut pages = Vec::new();
    for query in queries() {
        let result = client
            .catalog_page(&epoch, query.clone(), true)
            .await
            .unwrap();
        let cached = client.catalog_page(&epoch, query, false).await.unwrap();
        assert_eq!(result.epoch, epoch);
        let fresh = serde_json::to_value(result.value.unwrap()).unwrap();
        let saved = serde_json::to_value(cached.value.unwrap()).unwrap();
        assert_eq!(fresh["page"], saved["page"]);
        assert_eq!(fresh["revision"], saved["revision"]);
        assert_eq!(saved["cached"], true);
        let encoded = fresh.to_string();
        assert!(!encoded.contains("PRIVATE") && !encoded.contains("example.invalid"));
        assert!(
            !encoded.contains("cookie")
                && !encoded.contains("\"privilege\"")
                && !encoded.contains("shareUrl")
        );
        pages.push(fresh["page"].clone());
    }
    assert_eq!(pages[0]["artist"]["aliases"], json!(["译名", "别名"]));
    assert_eq!(
        pages[0]["artist"]["coverUrl"],
        "https://p2.music.126.net/a.jpg?param=320y320"
    );
    assert_eq!(pages[0]["tracks"][1]["id"], "8");
    assert_eq!(pages[0]["tracks"][1]["missing"], true);
    assert_eq!(pages[1]["followed"], false);
    assert_eq!(pages[1]["videoCount"], 3);
    assert_eq!(pages[2]["offset"], 20);
    assert_eq!(pages[2]["total"], 50);
    assert_eq!(pages[3]["albums"][0]["subscribed"], Value::Null);
    assert_eq!(pages[4]["sections"][0]["text"], "一\n二");
    assert_eq!(pages[5]["more"], false); // live/official similar response may omit more
    let videos = pages[6]["videos"].as_array().unwrap();
    assert_eq!(videos[0]["mvId"], "20");
    assert_eq!(videos[1]["kind"], "video");
    assert_eq!(videos[1]["mvId"], Value::Null);
    assert_eq!(videos[2]["kind"], "unknown");
    assert_eq!(videos[2]["mvId"], Value::Null);
    assert_eq!(videos[3]["mvId"], "23");
    assert_eq!(videos[4]["mvId"], Value::Null);
    assert_eq!(pages[6]["nextCursor"], "90071992547409931234");
    assert_eq!(pages[7]["more"], true);
    assert_eq!(pages[7]["concerts"][0]["price"], "88.00 起");
    assert_eq!(pages[7]["concerts"][0]["start"], 1234);
    assert_eq!(pages[7]["concerts"][0]["end"], "2026-10-28");
    assert_eq!(pages[8]["tracks"][0]["albumId"], "7");
    assert_eq!(pages[8]["album"]["subscribed"], false);
    assert_eq!(
        pages[8]["read"],
        json!({"source":"v3","privilegesRefreshed":true,"digitalAlbumTag":true})
    );
}
#[tokio::test]
async fn absent_snapshots_are_explicit_and_accounts_cannot_read_each_others_pages() {
    let (_root, api, session, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let query = CatalogQuery::Artist { id: "12".into() };
    assert!(
        client
            .catalog_page(&epoch, query.clone(), false)
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert!(api.calls.lock().unwrap().is_empty());
    client
        .catalog_page(&epoch, query.clone(), true)
        .await
        .unwrap();
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.catalog_page(&epoch, query.clone(), false).await,
        Err(Error::StaleResponse)
    ));
    let next = session.token().unwrap().generation.to_string();
    assert!(
        client
            .catalog_page(&next, query.clone(), false)
            .await
            .unwrap()
            .value
            .is_none()
    );
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(
        client
            .catalog_page(
                &session.token().unwrap().generation.to_string(),
                query,
                false
            )
            .await
            .unwrap()
            .value
            .is_some()
    );
}
#[tokio::test]
async fn invalid_media_records_fail_before_replacing_known_snapshots() {
    let (_root, api, session, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    for (query, path, pointer) in [
        (
            CatalogQuery::ArtistVideos {
                id: "12".into(),
                cursor: "0".into(),
                size: 20,
            },
            "/api/mlog/artist/video",
            "/data/records/0/id",
        ),
        (
            CatalogQuery::ArtistConcerts {
                id: "12".into(),
                offset: 0,
                limit: 20,
            },
            "/api/concert/list/byartist",
            "/simpleConcerts/0/id",
        ),
    ] {
        let before = serde_json::to_value(
            client
                .catalog_page(&epoch, query.clone(), true)
                .await
                .unwrap(),
        )
        .unwrap();
        *api.responses
            .lock()
            .unwrap()
            .get_mut(path)
            .unwrap()
            .pointer_mut(pointer)
            .unwrap() = json!("https://example.invalid/PRIVATE");
        assert!(
            client
                .catalog_page(&epoch, query.clone(), true)
                .await
                .is_err()
        );
        let after =
            serde_json::to_value(client.catalog_page(&epoch, query, false).await.unwrap()).unwrap();
        assert_eq!(before["value"]["page"], after["value"]["page"]);
    }
}
#[tokio::test]
async fn account_switch_drops_a_late_network_result_at_the_client_boundary() {
    let (_root, api, session, client) = fixture();
    api.hold.store(true, Ordering::SeqCst);
    let epoch = session.token().unwrap().generation.to_string();
    let future = tokio::spawn(async move {
        client
            .catalog_page(
                &epoch,
                CatalogQuery::ArtistDynamic { id: "12".into() },
                true,
            )
            .await
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(future.await.unwrap(), Err(Error::StaleResponse)));
}
