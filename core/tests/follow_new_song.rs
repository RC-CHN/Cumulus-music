use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
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

const LIST: &str = "/api/sub/artist/new/works/song-mv/list/v2";
const LATEST: &str = "/api/sub/artist/new/works/song/playall";

#[derive(Default)]
struct Fixture {
    calls: Mutex<Vec<(String, Value, bool)>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), body.clone(), cache));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            let song = json!({"id":"9007199254740993","name":"合成单曲","ar":[{"id":16,"name":"合成歌手"}],
                "al":{"id":17,"name":"合成专辑","picUrl":"https://p1.music.126.net/a.jpg?token=PRIVATE"},
                "mv":"998","url":"https://example.invalid/PRIVATE","credentials":"PRIVATE"});
            match path {
                LATEST if body == json!({}) => Ok(json!({"code":200,"data":{"songList":[song]}})),
                LIST if body["firstRequest"] == true => Ok(json!({"code":200,"data":{
                "latestVisitTime":925,"hasMore":true,"newWorks":[
                  {"sourceType":1,"info":{"blockType":"song","publishTime":950,
                    "blockTitle":{"artistId":16,"artistName":"合成歌手","resourceId":19,
                    "imgUrl":"https://p1.music.126.net/b.jpg?auth=PRIVATE"},"songLists":[song]}},
                  {"sourceType":1,"info":{"blockType":"unknown","publishTime":900}},
                  {"sourceType":2,"info":{"blockType":"album","publishTime":800,"albumId":17}}
                ]}})),
                LIST if body["startTimestamp"] == 900 => Ok(json!({"code":200,"data":{
                "latestVisitTime":7,"hasMore":false,"newWorks":[
                  {"sourceType":1,"info":{"blockType":"album","publishTime":870,"albumId":17,
                    "albumSongCount":8,"blockTitle":{"artistId":16,"artistName":"合成歌手",
                    "resourceName":"合成专辑"},"songLists":[song,{"id":7,"name":"受限歌曲","mv":44,"privilege":{"st":-100}}]}}
                ]}})),
                LIST => Ok(json!({"code":200,"data":{"hasMore":true,"newWorks":[]}})),
                _ => Err(Error::Response("unexpected followed releases request")),
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
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Fixture::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (dir, session, api, client)
}

#[tokio::test]
async fn list_and_independent_play_all_preserve_exact_cursor_song_mv_and_private_boundary() {
    let (dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let latest = client.followed_new_song_latest(&epoch).await.unwrap();
    let first = client.followed_new_song_page(&epoch, None).await.unwrap();
    let next = client
        .followed_new_song_page(&epoch, Some(900))
        .await
        .unwrap();
    let first = serde_json::to_value(first).unwrap();
    let next = serde_json::to_value(next).unwrap();
    let latest = serde_json::to_value(latest).unwrap();
    let calls = api.calls.lock().unwrap();
    let start = calls[1].1["startTimestamp"].as_u64().unwrap();
    assert!(start > 950);
    assert_eq!(
        *calls,
        vec![
            (LATEST.into(), json!({}), false),
            (
                LIST.into(),
                json!({"startTimestamp":start,"firstRequest":true,"sourceType":1,"limit":10}),
                false
            ),
            (
                LIST.into(),
                json!({"startTimestamp":900,"firstRequest":false,"sourceType":1,"limit":10}),
                false
            ),
        ]
    );
    assert_eq!(first["value"]["latestVisitTime"], 925);
    assert_eq!(first["value"]["nextTimestamp"], 900);
    assert_eq!(first["value"]["releases"].as_array().unwrap().len(), 1);
    assert_eq!(first["value"]["releases"][0]["kind"], "song");
    assert_eq!(
        first["value"]["releases"][0]["tracks"][0]["song"]["id"],
        "9007199254740993"
    );
    assert_eq!(first["value"]["releases"][0]["tracks"][0]["mvId"], "998");
    assert_eq!(next["value"]["releases"][0]["kind"], "album");
    assert_eq!(next["value"]["releases"][0]["albumId"], "17");
    assert!(next["value"]["releases"][0]["tracks"][1]["mvId"].is_null());
    assert!(next["value"]["latestVisitTime"].is_null());
    assert!(next["value"]["nextTimestamp"].is_null());
    assert_eq!(latest["value"][0]["id"], "9007199254740993");
    assert!(
        ![first, next, latest]
            .iter()
            .any(|reply| reply.to_string().contains("PRIVATE"))
    );
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    assert!(
        store
            .get("9", "follow_new_song", "last_enter_ms")
            .unwrap()
            .unwrap()
            .as_u64()
            .unwrap()
            >= start
    );
    assert!(
        store
            .get("10", "follow_new_song", "last_enter_ms")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stale_accounts_and_stalled_continuations_cannot_surface_as_success() {
    let (dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    assert!(matches!(
        client.followed_new_song_page(&epoch, Some(0)).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    assert!(matches!(
        client.followed_new_song_page(&epoch, Some(700)).await,
        Err(Error::Response(_))
    ));
    api.hold.store(true, Ordering::SeqCst);
    let client2 = client.clone();
    let old_epoch = epoch.clone();
    let read = tokio::spawn(async move { client2.followed_new_song_page(&old_epoch, None).await });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(
        client.followed_new_song_latest(&epoch).await,
        Err(Error::StaleResponse)
    ));
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    assert!(
        store
            .get("9", "follow_new_song", "last_enter_ms")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get("10", "follow_new_song", "last_enter_ms")
            .unwrap()
            .is_none()
    );
}
