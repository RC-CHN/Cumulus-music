#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    history::{HistoryKind, HistoryRecord, HistoryService},
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
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    deny: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
fn song(id: &Value) -> Value {
    json!({"id":id,"name":"歌曲","dt":120000,"ar":[{"id":12,"name":"创作者"}],"al":{"id":7,"name":"专辑","picUrl":"https://p1.music.126.net/image.jpg?token=PRIVATE"},"url":"https://example.invalid/PRIVATE","cookie":"PRIVATE"})
}
fn radio(id: &Value) -> Value {
    json!({"id":id,"name":"播客或书","programCount":2,"categoryId":if id=="10" {10001}else{1},"picUrl":"https://p1.music.126.net/image.jpg?token=PRIVATE"})
}
fn voice(id: u64, radio_id: &Value) -> Value {
    json!({"id":id,"name":"声音","mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":radio(radio_id),"url":"https://example.invalid/PRIVATE"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            Ok(match path {
                "/api/v3/song/detail" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let ids: Vec<Value> = serde_json::from_str(p["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|v|song(&v["id"])).collect::<Vec<_>>()})
                }
                "/api/song/enhance/privilege" => {
                    if self.deny.load(Ordering::SeqCst) {
                        return Err(Error::Transport("privilege read failed".into()));
                    }
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.into_iter().map(|id|json!({"id":id,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/album/privilege" => {
                    json!({"code":200,"data":[{"id":1,"pl":320000},{"id":2,"pl":320000}]})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"专辑"},"songs":[song(&json!(1)),song(&json!(2))]})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"trackIds":[{"id":1},{"id":3,"v":-1},{"id":2}]}})
                }
                "/api/djradio/v3/get" => json!({"code":200,"data":radio(&p["id"])}),
                "/api/v6/dj/program/byradio" => {
                    json!({"code":200,"data":{"programs":[voice(5,&p["radioId"]),voice(6,&p["radioId"])],"count":2,"more":false,"asc":p["asc"]}})
                }
                "/api/dj/program/detail" => json!({"code":200,"program":voice(5,&json!(8))}),
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/play/url/check" => {
                    json!({"code":200,"data":{"voiceId":p["voiceId"],"songId":1005,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/synthetic.mp3?key=PRIVATE","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/song/lyric/v1" => json!({"code":200,"nolyric":true}),
                _ => panic!("unexpected history test endpoint: {path}"),
            })
        })
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    client: Arc<MusicClient>,
    history: HistoryService,
}
fn fixture(authenticated: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    if authenticated {
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
    }
    let history = HistoryService::new(session.clone()).unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    Fixture {
        _root: root,
        api,
        session,
        client: Arc::new(client),
        history,
    }
}
impl Fixture {
    fn seed(&self, kind: HistoryKind) -> String {
        let raw = match kind {
            HistoryKind::Song => song(&json!("90071992547409931234")),
            HistoryKind::Album => {
                json!({"id":7,"name":"专辑","artists":[{"id":12,"name":"创作者"}],"picUrl":"https://p1.music.126.net/image.jpg?auth=PRIVATE"})
            }
            HistoryKind::Playlist => {
                json!({"id":6,"name":"歌单","creator":{"nickname":"创作者"},"coverImgUrl":"https://p1.music.126.net/image.jpg?auth=PRIVATE"})
            }
            HistoryKind::Voice => voice(5, &json!(8)),
            HistoryKind::Podcast => radio(&json!(8)),
            HistoryKind::Audiobook => radio(&json!("10")),
        };
        let row = HistoryRecord::local(kind, raw, 100).unwrap();
        let id = row.id.clone();
        self.history
            .record_for(&self.session.token().unwrap(), kind, row)
            .unwrap();
        id
    }
    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }
    async fn selection(&self, kind: HistoryKind, ids: Vec<String>) -> PlaySelection {
        let page = self
            .client
            .history_page(&self.epoch(), kind, false)
            .await
            .unwrap()
            .value;
        PlaySelection::History {
            history_kind: kind,
            revision: page.revision,
            ids,
            index: 0,
            quality: Quality::Standard,
        }
    }
}
#[tokio::test]
async fn six_typed_pages_redact_native_secrets_preserve_large_ids_and_isolate_accounts() {
    let f = fixture(true);
    for kind in HistoryKind::ALL {
        let id = f.seed(kind);
        let page = f
            .client
            .history_page(&f.epoch(), kind, false)
            .await
            .unwrap();
        let data = serde_json::to_value(page.value).unwrap();
        assert_eq!(data["entries"][0]["id"], id);
        let encoded = data.to_string();
        assert!(
            !encoded.contains("PRIVATE")
                && !encoded.contains("example.invalid")
                && !encoded.contains("cookie")
        );
        assert_eq!(data["entries"][0]["device"], "本机");
    }
    assert!(f.api.calls.lock().unwrap().is_empty());
    let old = f.epoch();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.history_page(&old, HistoryKind::Song, false).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .history_page(&f.epoch(), HistoryKind::Song, false)
            .await
            .unwrap()
            .value
            .entries
            .is_empty()
    );
}
#[tokio::test]
async fn six_history_routes_expand_real_native_queues_and_refresh_rights() {
    for kind in HistoryKind::ALL {
        let f = fixture(true);
        let id = f.seed(kind);
        let selected = f.selection(kind, vec![id]).await;
        let state = f.client.play_selection(&f.epoch(), selected).await.unwrap();
        assert_eq!(
            state.queue_len,
            if matches!(kind, HistoryKind::Song | HistoryKind::Voice) {
                1
            } else {
                2
            }
        );
        assert!(state.error.is_none());
        assert_eq!(serde_json::to_value(state.state).unwrap(), "playing");
        assert!(!serde_json::to_string(&state).unwrap().contains("PRIVATE"));
        let calls = f.api.calls.lock().unwrap();
        if matches!(
            kind,
            HistoryKind::Song | HistoryKind::Album | HistoryKind::Playlist
        ) {
            assert!(calls.iter().any(|c| c == "/api/song/enhance/privilege"));
        } else {
            assert!(calls.iter().any(|c| c == "/api/dj/play/url/check"));
        }
    }
}
#[tokio::test]
async fn stale_foreign_duplicate_and_reordered_history_selections_fail_before_metadata_reads() {
    for mode in ["stale", "foreign", "duplicate", "order"] {
        let f = fixture(true);
        let first = f.seed(HistoryKind::Song);
        f.history
            .record_for(
                &f.session.token().unwrap(),
                HistoryKind::Song,
                HistoryRecord::local(HistoryKind::Song, song(&json!(2)), 99).unwrap(),
            )
            .unwrap();
        let mut selected = f
            .selection(HistoryKind::Song, vec![first.clone(), "2".into()])
            .await;
        if let PlaySelection::History { revision, ids, .. } = &mut selected {
            match mode {
                "stale" => *revision = "stale".into(),
                "foreign" => *ids = vec!["404".into()],
                "duplicate" => *ids = vec![first.clone(), first],
                _ => ids.reverse(),
            }
        }
        assert!(f.client.play_selection(&f.epoch(), selected).await.is_err());
        assert!(f.api.calls.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn failed_privilege_refresh_cannot_replace_a_playing_queue() {
    let f = fixture(true);
    let id = f.seed(HistoryKind::Song);
    let state = f
        .client
        .play_selection(
            &f.epoch(),
            f.selection(HistoryKind::Song, vec![id.clone()]).await,
        )
        .await
        .unwrap();
    f.api.deny.store(true, Ordering::SeqCst);
    assert!(
        f.client
            .play_selection(&f.epoch(), f.selection(HistoryKind::Song, vec![id]).await)
            .await
            .is_err()
    );
    let after = f
        .client
        .subscribe_playback(&f.epoch())
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(state.entry_id, after.entry_id);
    assert_eq!(state.queue_len, after.queue_len);
}
#[tokio::test]
async fn account_switch_during_history_metadata_read_discards_late_queue() {
    let f = fixture(true);
    let id = f.seed(HistoryKind::Song);
    let selected = f.selection(HistoryKind::Song, vec![id]).await;
    f.api.hold.store(true, Ordering::SeqCst);
    let client = f.client.clone();
    let epoch = f.epoch();
    let task = tokio::spawn(async move { client.play_selection(&epoch, selected).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), f.api.started.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
}
#[tokio::test]
async fn guest_history_does_not_request_remote_resources() {
    let f = fixture(false);
    let page = f
        .client
        .history_page(&f.epoch(), HistoryKind::Song, true)
        .await
        .unwrap()
        .value;
    assert!(!page.can_sync && page.entries.is_empty());
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unusable_download_reference_falls_back_only_through_fresh_online_metadata_and_rights() {
    let f = fixture(true);
    let mut resource = song(&json!(1));
    resource["download_task_id"] = json!(uuid::Uuid::new_v4().to_string());
    f.history
        .record_for(
            &f.session.token().unwrap(),
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, resource, 100).unwrap(),
        )
        .unwrap();
    let state = f
        .client
        .play_selection(
            &f.epoch(),
            f.selection(HistoryKind::Song, vec!["1".into()]).await,
        )
        .await
        .unwrap();
    assert_eq!(state.track.unwrap().id, "1");
    let calls = f.api.calls.lock().unwrap();
    let rights = calls
        .iter()
        .position(|p| p == "/api/song/enhance/privilege")
        .unwrap();
    let media = calls
        .iter()
        .position(|p| p == "/api/song/enhance/player/url/v1")
        .unwrap();
    assert!(rights < media);
}
