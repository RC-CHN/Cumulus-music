#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    hold_id: AtomicU64,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            Ok(match path {
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"长专辑"},"songs":(1..=501).map(|id|json!({"id":id,"name":"曲目","dt":120000})).collect::<Vec<_>>()})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"trackIds":(1..=205).map(|id|json!({"id":id})).collect::<Vec<_>>()}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let id = ids[0]["id"].as_str().unwrap().parse::<u64>().unwrap();
                    if self.hold_id.load(Ordering::SeqCst) == id {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"songs":ids.iter().map(|v|json!({"id":v["id"],"name":"曲目","ar":[{"name":"歌手"}],"al":{"name":"专辑","picUrl":"http://p1.music.126.net/image.jpg?token=SECRET"},"dt":120000,"url":"https://private/?secret=SECRET","cookie":"SECRET"})).collect::<Vec<_>>()})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/a.mp3?key=SECRET","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/song/lyric/v1" => {
                    json!({"code":200,"lrc":{"lyric":"[00:01]第一行\n[00:03]第二行","version":1},"yrc":{"lyric":"[1000,1000](1000,500,0)你(1500,500,0)好","version":1},"ytlrc":{"lyric":"[1000,1000]hello"},"yromalrc":{"lyric":"[1000,1000]ni hao"},"lyricContributor":{"cookie":"SECRET"}})
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
    Arc<AtomicU64>,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let calls = count.clone();
    let client =
        MusicClient::with_session(session.clone(), registry).with_audio_factory(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(support::audio_engine(Arc::new(support::Device::default())))
        });
    (dir, api, session, Arc::new(client), count)
}
fn songs(id: &str) -> PlaySelection {
    PlaySelection::Songs {
        ids: vec![id.into()],
        index: 0,
        quality: Quality::Lossless,
    }
}

#[tokio::test]
async fn album_selection_uses_complete_native_order_and_rejects_changed_revision() {
    let (_dir, _api, _session, client, _count) = fixture();
    let page = client
        .ranking_page("1", RankingQuery::Album { id: "7".into() }, true)
        .await
        .unwrap();
    let RankingPageView::Album {
        revision, tracks, ..
    } = page.value.page
    else {
        panic!()
    };
    assert_eq!(tracks.len(), 501);
    let state = client
        .play_selection(
            "1",
            PlaySelection::Album {
                id: "7".into(),
                start_id: Some("501".into()),
                revision,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 501);
    assert_eq!(state.track.unwrap().id, "501");
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Album {
                    id: "7".into(),
                    start_id: None,
                    revision: "changed".into(),
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 501);
    assert_eq!(queue.tracks.last().unwrap().track.id, "501");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn complete_playlist_is_native_and_public_player_queue_lyrics_are_whitelisted() {
    let (dir, api, _session, client, count) = fixture();
    let mut subscription = client.subscribe_playback("1").await.unwrap();
    let view = client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: Some("151".into()),
                revision: None,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.queue_len, 205);
    assert_eq!(view.track.as_ref().unwrap().id, "151");
    assert_eq!(view.requested_quality, Some("lossless".into()));
    assert_eq!(view.actual_quality, Some("standard".into()));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "/api/v3/song/detail")
            .count(),
        3
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 205);
    assert_eq!(queue.tracks[204].track.id, "205");
    let id = view.play_id.as_ref().unwrap();
    let lyric = client.player_lyrics("1", id).await.unwrap();
    assert_eq!(lyric.lines.len(), 2);
    assert_eq!(lyric.word_lines[0].words.len(), 2);
    assert_eq!(lyric.word_translation[0].lyric.trim(), "hello");
    assert_eq!(lyric.word_romanization[0].lyric.trim(), "ni hao");
    let changed = client.lyric_offset("1", id, 1.0).await.unwrap();
    assert_eq!(changed.offset, 0.5);
    assert_eq!(changed.lines[0].time, Some(0.5));
    for value in [
        serde_json::to_value(view).unwrap(),
        serde_json::to_value(queue).unwrap(),
        serde_json::to_value(lyric).unwrap(),
    ] {
        let text = value.to_string();
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("cookie"));
        assert!(!text.contains("resource_info"));
    }
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert!(!stored.to_string().contains("SECRET"));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let updated = subscription.changed().await.unwrap();
    assert_eq!(
        updated.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
}

#[tokio::test]
async fn stale_inputs_and_unknown_native_fields_never_initialize_audio_or_access_network() {
    let (_dir, api, session, client, count) = fixture();
    assert!(matches!(
        client.play_selection("0", songs("1")).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert!(matches!(
        client
            .player_action("0", PlayerAction::Play)
            .await
            .err()
            .unwrap(),
        Error::StaleResponse
    ));
    assert!(serde_json::from_value::<PlaySelection>(json!({"kind":"songs","ids":["1"],"index":0,"quality":"standard","url":"https://private"})).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
    let stream = client.subscribe_playback("1").await.unwrap();
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        stream.current().err().unwrap(),
        Error::StaleResponse
    ));
}

#[tokio::test]
async fn slow_metadata_cannot_replace_new_selection_and_old_lyrics_cannot_adjust_new_song() {
    let (_dir, api, _session, client, _) = fixture();
    api.hold_id.store(1, Ordering::SeqCst);
    let old_client = client.clone();
    let old = tokio::spawn(async move { old_client.play_selection("1", songs("1")).await });
    api.started.notified().await;
    let next = client.play_selection("1", songs("2")).await.unwrap();
    api.release.notify_waiters();
    assert!(matches!(
        old.await.unwrap().err().unwrap(),
        Error::StaleResponse
    ));
    let play_id = next.play_id.unwrap();
    client.player_lyrics("1", &play_id).await.unwrap();
    client.play_selection("1", songs("3")).await.unwrap();
    assert!(matches!(
        client.lyric_offset("1", &play_id, 1.0).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().tracks[0].track.id,
        "3"
    );
}
