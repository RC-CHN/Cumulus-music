use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::rankings::*,
    client::{MusicClient, dto::RankingPageView},
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
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
fn product(id: &str) -> Value {
    json!({"albumId":id,"albumName":"专辑","artistName":"歌手","rank":0,"rankIncr":-1,"saleNum":4,"coverUrl":"http://p1.music.126.net/a.jpg?secret=PRIVATE"})
}
fn song(id: &str) -> Value {
    json!({"id":id,"name":"曲目","ar":[{"name":"歌手"}],"al":{"name":"专辑","picUrl":"http://p1.music.126.net/a.jpg?key=PRIVATE"},"dt":3000,"url":"PRIVATE","cookie":"PRIVATE"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::Response("fixture read failure"));
            }
            Ok(match path {
                "/api/toplist/detail/v2" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"data":[
                    {"name":"官方榜","categoryCode":"OFFICIAL","displayType":"TOP_3","list":[
                        {"id":"9007199254740993123","name":"新榜","nameShowStyle":"singleRow","targetType":"PLAYLIST","canPlay":true,"coverUrl":"https://evil.test/PRIVATE","trackRankList":[{"trackId":22,"rank":1,"lastRank":0,"songName":"fallback","coverImgUrl":"http://p1.music.126.net/a.jpg?key=PRIVATE"}],"tracks":[{"first":"","second":null}]},
                        {"id":0,"nameShowStyle":"singleRow","targetType":"H5"},
                        {"id":1,"nameShowStyle":"unsupported","targetType":"PLAYLIST"}]},
                    {"name":"unsupported","displayType":"VIDEO"},
                    {"name":"empty","displayType":"ONLY_COVER","list":[]}]})
                }
                "/api/feealbum/songsaleboard/daily/type" => {
                    json!({"code":200,"products":[product("7")]})
                }
                "/api/single/sales-ranking/get" => {
                    json!({"code":200,"data":{"genTime":123456789,"itemList":[{"songId":22,"songName":"缺失曲目","rankIncr":-3,"indicator":50.2},{"songId":11,"songName":"完整曲目","rankIncr":4}]}})
                }
                "/api/v3/song/detail" => json!({"code":200,"songs":[song("11")]}),
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"专辑","picUrl":"http://p1.music.126.net/a.jpg?key=PRIVATE","cookie":"PRIVATE"},"songs":[song("22"),song("11")]})
                }
                "/api/batch" => {
                    let mut response = json!({"code":200});
                    for (path, _) in payload.as_object().unwrap() {
                        response[path] = json!({"code":200,"products":if path.ends_with('/') {vec![]}else{vec![product("7")]}});
                    }
                    response
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
    Arc<MusicClient>,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (dir, api, session, client)
}
#[tokio::test]
async fn official_group_filtering_special_destinations_and_public_projection() {
    let (_dir, _api, _session, client) = fixture();
    let v = client
        .ranking_page("1", RankingQuery::Home, true)
        .await
        .unwrap();
    let safe = serde_json::to_string(&v).unwrap();
    assert!(!safe.contains("PRIVATE") && !safe.contains("evil.test"));
    let RankingPageView::Home { groups } = v.value.page else {
        panic!()
    };
    assert_eq!(groups.len(), 1);
    let cards = &groups[0].cards;
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].id, "9007199254740993123");
    assert_eq!(cards[0].tracks[0].name, "");
    assert_eq!(cards[0].tracks[0].last_rank, Some(0));
    assert!(cards[0].can_play);
    assert_eq!(cards[1].target, RankingTarget::AlbumSales);
    assert!(!cards[1].can_play);
    assert_eq!(cards[1].tracks[0].last_rank, Some(-1));
    assert_eq!(cards[2].target, RankingTarget::SongSales);
}
#[tokio::test]
async fn official_batch_keeps_slash_keys_types_all_periods_and_historical_year() {
    let (_dir, api, session, _client) = fixture();
    let service = Rankings::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    let full = service
        .page_for(&token, &RankingQuery::AlbumSales { year: 2026 })
        .await
        .unwrap();
    let RankingPage::AlbumSales { boards, .. } = full.page else {
        panic!()
    };
    assert_eq!(boards.len(), 8);
    assert!(
        boards
            .iter()
            .filter(|b| b.singles)
            .all(|b| b.products.is_empty())
    );
    service
        .page_for(&token, &RankingQuery::AlbumYear { year: 2018 })
        .await
        .unwrap();
    {
        let calls = api.calls.lock().unwrap();
        assert_eq!(calls[0].0, "/api/batch");
        assert_eq!(calls[0].1.as_object().unwrap().len(), 8);
        for (path, v) in calls[0].1.as_object().unwrap() {
            let p: Value = serde_json::from_str(v.as_str().unwrap()).unwrap();
            assert_eq!(p["limit"], 50);
            assert_eq!(p["albumType"], u8::from(path.ends_with('/')));
            assert_eq!(p.get("year").is_some(), path.contains("/year/"));
        }
        assert_eq!(calls[1].1.as_object().unwrap().len(), 2);
        for v in calls[1].1.as_object().unwrap().values() {
            let p: Value = serde_json::from_str(v.as_str().unwrap()).unwrap();
            assert_eq!(p["year"], 2018);
        }
    }
    assert!(matches!(
        service
            .page_for(&token, &RankingQuery::AlbumYear { year: 2017 })
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(api.calls.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn sales_order_missing_details_and_album_revision_are_preserved() {
    let (_dir, _api, _session, client) = fixture();
    let sales = client
        .ranking_page("1", RankingQuery::SongSales, true)
        .await
        .unwrap();
    assert!(!serde_json::to_string(&sales).unwrap().contains("PRIVATE"));
    let RankingPageView::SongSales { songs, .. } = sales.value.page else {
        panic!()
    };
    assert_eq!(
        songs
            .iter()
            .map(|s| s.track.id.as_str())
            .collect::<Vec<_>>(),
        ["22", "11"]
    );
    assert!(songs[0].track.missing);
    assert!(!songs[1].track.missing);
    assert_eq!(songs[0].indicator, Some(50.2));
    let album = client
        .ranking_page("1", RankingQuery::Album { id: "7".into() }, true)
        .await
        .unwrap();
    assert!(!serde_json::to_string(&album).unwrap().contains("PRIVATE"));
    let RankingPageView::Album {
        tracks, revision, ..
    } = album.value.page
    else {
        panic!()
    };
    assert_eq!(tracks.len(), 2);
    assert_eq!(revision.len(), 64);
}
#[tokio::test]
async fn cached_page_survives_read_failure_and_store_reopen_but_is_account_isolated() {
    let (dir, api, session, client) = fixture();
    let first = client
        .ranking_page("1", RankingQuery::Home, true)
        .await
        .unwrap();
    api.fail.store(true, Ordering::SeqCst);
    assert!(
        client
            .ranking_page("1", RankingQuery::Home, true)
            .await
            .is_err()
    );
    let saved = Rankings::new(session.clone())
        .unwrap()
        .cached_for(&session.token().unwrap(), &RankingQuery::Home)
        .unwrap()
        .unwrap();
    assert_eq!(first.value.fetched_at, saved.fetched_at);
    assert!(saved.fresh_at(saved.fetched_at + 10_797_000));
    assert!(!saved.fresh_at(saved.fetched_at + 10_797_001));
    assert!(!saved.fresh_at(saved.fetched_at - 1));
    let cached = client
        .ranking_page("1", RankingQuery::Home, false)
        .await
        .unwrap();
    assert!(cached.value.cached);
    let reopened = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened_session = Arc::new(AccountSession::new(api.clone(), Some(reopened.store())));
    reopened_session
        .adopt(
            api.clone(),
            json!({"userId":9}),
            &reopened_session.token().unwrap(),
        )
        .unwrap();
    let reopened_cache = Rankings::new(reopened_session.clone())
        .unwrap()
        .cached_for(&reopened_session.token().unwrap(), &RankingQuery::Home)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(reopened_cache).unwrap()
    );
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.ranking_page("1", RankingQuery::Home, false).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        Rankings::new(session.clone())
            .unwrap()
            .cached_for(&session.token().unwrap(), &RankingQuery::Home)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn account_change_rejects_delayed_network_result_and_does_not_cache_it() {
    let (_dir, api, session, client) = fixture();
    api.hold.store(true, Ordering::SeqCst);
    let task = tokio::spawn({
        let client = client.clone();
        async move { client.ranking_page("1", RankingQuery::Home, true).await }
    });
    api.started.notified().await;
    assert!(matches!(
        client.ranking_page("1", RankingQuery::Home, true).await,
        Err(Error::MutationBusy)
    ));
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(
        Rankings::new(session.clone())
            .unwrap()
            .cached_for(&session.token().unwrap(), &RankingQuery::Home)
            .unwrap()
            .is_none()
    );
}
