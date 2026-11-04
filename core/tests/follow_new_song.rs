use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{
            PlaySelection, ReleaseNotificationAction as NoticeAction,
            ReleaseNotificationState as NoticeState,
        },
    },
    membership::Quality,
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
const SWITCH: &str = "/api/sub/artist/publish/switch";

#[derive(Default)]
struct Fixture {
    calls: Mutex<Vec<(String, Value, bool)>>,
    hold: AtomicBool,
    deny_first: AtomicBool,
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
                SWITCH if body == json!({"artistId":"16","switchStatus":0}) => {
                    Ok(json!({"code":200}))
                }
                SWITCH if body == json!({"artistId":"17","switchStatus":0}) => {
                    Err(Error::Transport("synthetic acknowledgement lost".into()))
                }
                SWITCH if body == json!({"artistId":"18","switchStatus":0}) => {
                    Ok(json!({"code":400}))
                }
                LATEST if body == json!({}) => {
                    if self.deny_first.load(Ordering::SeqCst) {
                        let mut denied = song;
                        denied["privilege"] = json!({"pl":0,"flag":0});
                        return Ok(
                            json!({"code":200,"data":{"songList":[denied,{"id":42,"name":"可播后续","privilege":{"pl":128000}}]}}),
                        );
                    }
                    Ok(json!({"code":200,"data":{"songList":[song]}}))
                }
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

#[tokio::test]
async fn explicit_notice_disable_uses_only_the_switch_and_confirms_once() {
    let (_dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .release_notification(&epoch, "16", NoticeAction::Inspect)
            .await
            .unwrap()
            .value,
        NoticeState::Ready
    );
    assert!(api.calls.lock().unwrap().is_empty());
    api.hold.store(true, Ordering::SeqCst);
    let active = {
        let client = client.clone();
        let epoch = epoch.clone();
        tokio::spawn(async move {
            client
                .release_notification(&epoch, "16", NoticeAction::Disable)
                .await
        })
    };
    api.started.notified().await;
    assert_eq!(
        client
            .release_notification(&epoch, "16", NoticeAction::Disable)
            .await
            .unwrap()
            .value,
        NoticeState::Uncertain
    );
    api.release.notify_one();
    assert_eq!(active.await.unwrap().unwrap().value, NoticeState::Confirmed);
    assert_eq!(
        client
            .release_notification(&epoch, "16", NoticeAction::Disable)
            .await
            .unwrap()
            .value,
        NoticeState::Confirmed
    );
    assert_eq!(
        *api.calls.lock().unwrap(),
        [(
            SWITCH.into(),
            json!({"artistId":"16","switchStatus":0}),
            false
        )]
    );
}

#[tokio::test]
async fn notice_loss_stays_account_local_and_never_replays_but_service_rejection_is_retryable() {
    let (dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .release_notification(&epoch, "17", NoticeAction::Disable)
            .await
            .unwrap()
            .value,
        NoticeState::Uncertain
    );
    let reopened = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened_session = Arc::new(AccountSession::new(api.clone(), Some(reopened.store())));
    reopened_session
        .adopt(
            api.clone(),
            json!({"userId":9}),
            &reopened_session.token().unwrap(),
        )
        .unwrap();
    let reopened_epoch = reopened_session.token().unwrap().generation.to_string();
    let reopened = MusicClient::with_session(reopened_session, reopened);
    assert_eq!(
        reopened
            .release_notification(&reopened_epoch, "17", NoticeAction::Disable)
            .await
            .unwrap()
            .value,
        NoticeState::Uncertain
    );
    assert!(
        client
            .release_notification(&epoch, "18", NoticeAction::Disable)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .release_notification(&epoch, "18", NoticeAction::Inspect)
            .await
            .unwrap()
            .value,
        NoticeState::Ready
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client
            .release_notification(&epoch, "17", NoticeAction::Inspect)
            .await,
        Err(Error::StaleResponse)
    ));
    let next_epoch = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .release_notification(&next_epoch, "17", NoticeAction::Inspect)
            .await
            .unwrap()
            .value,
        NoticeState::Ready
    );
    assert_eq!(api.calls.lock().unwrap().len(), 2);
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
    assert_eq!(latest["value"]["tracks"][0]["id"], "9007199254740993");
    assert!(
        latest["value"]["revision"]
            .as_str()
            .is_some_and(|revision| revision.len() == 32)
    );
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

#[tokio::test]
async fn latest_selection_is_native_and_denied_head_cannot_fall_forward() {
    let (_dir, session, api, client) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let revision = |value| {
        serde_json::to_value(value).unwrap()["value"]["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let previous = revision(client.followed_new_song_latest(&epoch).await.unwrap());
    api.deny_first.store(true, Ordering::SeqCst);
    let denied = revision(client.followed_new_song_latest(&epoch).await.unwrap());
    let select = |revision| PlaySelection::FollowLatest {
        revision,
        quality: Quality::Standard,
    };
    assert!(matches!(
        client.play_selection(&epoch, select(previous)).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.play_selection(&epoch, select(denied.clone())).await,
        Err(Error::State(
            "the latest release's first song is unavailable"
        ))
    ));
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let next_epoch = session.token().unwrap().generation.to_string();
    assert!(matches!(
        client.play_selection(&next_epoch, select(denied)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![(LATEST.into(), json!({}), false); 2]
    );
}
