#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    playback::controller::PlaybackStatus,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    relay::{RelayPlaybackPhase, RelayService},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    plan: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    deny: AtomicBool,
    started: Notify,
    release: Notify,
}
fn song(id: &Value) -> Value {
    json!({"id":id,"name":"Synthetic song","dt":120000,"ar":[{"id":12,"name":"Synthetic artist"}],"al":{"id":7,"name":"Synthetic album"},"url":"https://example.invalid/PRIVATE"})
}
fn voice(id: &str) -> Value {
    json!({"id":id,"name":"Synthetic voice","mainSong":{"id":"1005","duration":120000},"duration":120000,"radio":{"id":8,"name":"Synthetic radio"}})
}
impl Backend {
    fn new(plan: Value) -> Self {
        Self {
            plan: Mutex::new(plan),
            calls: Mutex::default(),
            hold: AtomicBool::new(false),
            deny: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
        }
    }
    fn pulls(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == "/api/relay/play/pull")
            .count()
    }
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => {
                    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":"t2"}]})
                }
                "/api/relay/config/get" => json!({"code":200,"data":{"enable":true}}),
                "/api/relay/setting/show" => {
                    json!({"code":200,"data":{"groups":[{"deviceSettingList":[{"enable":true}]}]}})
                }
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":[{"generalizedObject":{"deviceId":"PRIVATE-device","reconnectTitle":"继续播放","resourceName":"Synthetic","opaque":"PRIVATE-token","url":"https://example.invalid/PRIVATE"}}]}})
                }
                "/api/relay/play/pull" => {
                    json!({"code":200,"data":self.plan.lock().unwrap().clone()})
                }
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
                        return Err(Error::Transport("synthetic permission read failure".into()));
                    }
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.iter().map(|id|json!({"id":id,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/dj/program/batch/detail" => {
                    let ids: Vec<u64> =
                        serde_json::from_str(p["programIds"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.iter().rev().map(|id|voice(&id.to_string())).collect::<Vec<_>>()})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":voice(p["id"].as_str().unwrap())})
                }
                "/api/dj/play/url/check" => {
                    json!({"code":200,"data":{"voiceId":p["voiceId"],"songId":"1005","canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/synthetic.mp3?PRIVATE","level":"standard","md5":"fixture","expi":120}]})
                }
                "/api/song/lyric/v1" => json!({"code":200,"nolyric":true}),
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"trackIds":[{"id":1},{"id":3,"v":-1},{"id":2}]}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7},"songs":[song(&json!(1)),song(&json!(2))]})
                }
                "/api/album/privilege" => {
                    json!({"code":200,"data":[{"id":1,"pl":320000},{"id":2,"pl":320000}]})
                }
                "/api/djradio/v3/get" => {
                    json!({"code":200,"data":{"id":8,"name":"Synthetic radio","programCount":2}})
                }
                "/api/v6/dj/program/byradio" => {
                    json!({"code":200,"data":{"programs":[voice("5"),voice("6")],"count":2,"more":false,"asc":p["asc"]}})
                }
                other => panic!("unexpected relay endpoint {other}"),
            })
        })
    }
}
struct World {
    _root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    client: Arc<MusicClient>,
    device: Arc<engine::Device>,
}
impl World {
    fn new(plan: Value) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let api = Arc::new(Backend::new(plan));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let device = Arc::new(engine::Device::default());
        let cloned = device.clone();
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry)
                .with_audio_factory(move || Ok(engine::audio_engine(cloned.clone()))),
        );
        Self {
            _root: root,
            store,
            api,
            session,
            client,
            device,
        }
    }
    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }
    async fn revision(&self) -> String {
        self.client
            .relay_view(&self.epoch(), true)
            .await
            .unwrap()
            .value
            .revision
    }
    async fn play(&self, id: &str, revision: &str) -> RelayView {
        self.client
            .relay_command(
                &self.epoch(),
                RelayCommand::Continue {
                    id: id.into(),
                    revision: revision.into(),
                    quality: Quality::Standard,
                },
            )
            .await
            .unwrap()
            .value
    }
    async fn playing(&self) -> PlayerView {
        let mut stream = self.client.subscribe_playback(&self.epoch()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let value = stream.current().unwrap();
                if matches!(value.state, PlaybackStatus::Playing | PlaybackStatus::Error) {
                    return value;
                }
                stream.changed().await.unwrap();
            }
        })
        .await
        .unwrap()
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn plan(kind: &str) -> Value {
    json!({"relayMode":"snapshot","resources":[{"id":"2","type":"song"},{"id":"5","type":"voice"}],"currentPlayState":{"resourceId":if kind=="song"{"2"}else{"5"},"resourceType":kind,"progress":12500},"playMode":"random"})
}

#[tokio::test]
async fn mixed_queue_native_playback_music_restarts_and_voice_resumes_without_leaking_alert_fields()
{
    for kind in ["song", "voice"] {
        let w = World::new(plan(kind));
        let revision = w.revision().await;
        let id = id();
        let view = w.play(&id, &revision).await;
        assert_eq!(view.operations[0].phase, RelayPlaybackPhase::Installed);
        assert!(!serde_json::to_string(&view).unwrap().contains("PRIVATE"));
        let playing = w.playing().await;
        assert_eq!(playing.state, PlaybackStatus::Playing);
        assert_eq!(playing.mode, cumulus_core::playback::PlaybackMode::Random);
        assert_eq!(
            playing.position,
            Some(if kind == "song" { 0.0 } else { 12.5 })
        );
        assert_eq!(playing.queue_len, 2);
        let before = w.device.state.lock().unwrap().loads;
        w.play(&id, &revision).await;
        assert_eq!(w.device.state.lock().unwrap().loads, before);
        assert_eq!(w.api.pulls(), 1);
        let queue = w.store.get("9", "playback", "queue").unwrap().unwrap();
        assert_eq!(queue["items"][0]["resource"]["id"], "2");
        assert_eq!(queue["items"][1]["resource"]["voice_id"], "5");
        assert_eq!(queue["items"][0]["source"]["relay_application"], id);
        assert!(
            !w.api
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(p, _)| p.ends_with("submit") || p.ends_with("update"))
        );
    }
}
#[tokio::test]
async fn all_official_collection_sources_expand_and_select_the_remote_current_resource() {
    for (kind, source, current, resource) in [
        ("playlist", "6", "2", "song"),
        ("album", "7", "2", "song"),
        ("podcast", "8", "6", "voice"),
    ] {
        let w = World::new(
            json!({"relayMode":"source","sourceType":kind,"sourceId":source,"resources":[],"currentPlayState":{"resourceId":current,"resourceType":resource,"progress":9000},"playMode":"single_loop"}),
        );
        let revision = w.revision().await;
        let view = w.play(&id(), &revision).await;
        assert_eq!(
            view.operations[0].phase,
            RelayPlaybackPhase::Installed,
            "{kind}"
        );
        let playing = w.playing().await;
        assert_eq!(playing.state, PlaybackStatus::Playing, "{kind}");
        assert_eq!(playing.queue_len, 2);
        let queue = w.store.get("9", "playback", "queue").unwrap().unwrap();
        assert_eq!(queue["items"][1]["entry_id"], queue["current"]);
        assert_eq!(queue["items"][0]["source"]["kind"], kind);
    }
}
#[tokio::test]
async fn permission_failure_retains_ready_pull_and_explicit_retry_never_pulls_twice() {
    let w = World::new(plan("song"));
    let revision = w.revision().await;
    let first = id();
    w.api.deny.store(true, Ordering::SeqCst);
    let view = w.play(&first, &revision).await;
    assert_eq!(view.operations[0].phase, RelayPlaybackPhase::Failed);
    assert!(view.operations[0].can_retry);
    assert_eq!(w.device.state.lock().unwrap().loads, 0);
    w.api.deny.store(false, Ordering::SeqCst);
    let retry = id();
    let view = w
        .client
        .relay_command(
            &w.epoch(),
            RelayCommand::Retry {
                id: retry.clone(),
                previous_id: first,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(
        view.operations
            .iter()
            .find(|r| r.id == retry)
            .unwrap()
            .phase,
        RelayPlaybackPhase::Installed
    );
    assert_eq!(w.api.pulls(), 1);
    assert_eq!(w.playing().await.state, PlaybackStatus::Playing);
}
#[tokio::test]
async fn queue_inspection_does_not_invalidate_remote_preparation_but_user_stop_does() {
    for stop in [false, true] {
        let w = World::new(plan("song"));
        let revision = w.revision().await;
        w.api.hold.store(true, Ordering::SeqCst);
        let client = w.client.clone();
        let epoch = w.epoch();
        let id = id();
        let copy = id.clone();
        let task = tokio::spawn(async move {
            client
                .relay_command(
                    &epoch,
                    RelayCommand::Continue {
                        id: copy,
                        revision,
                        quality: Quality::Standard,
                    },
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), w.api.started.notified())
            .await
            .unwrap();
        w.client.player_queue(&w.epoch()).await.unwrap();
        if stop {
            w.client
                .player_action(&w.epoch(), PlayerAction::Stop)
                .await
                .unwrap();
        }
        w.api.release.notify_one();
        let result = task.await.unwrap().unwrap();
        assert_eq!(
            result
                .value
                .operations
                .iter()
                .find(|r| r.id == id)
                .unwrap()
                .phase,
            if stop {
                RelayPlaybackPhase::Failed
            } else {
                RelayPlaybackPhase::Installed
            }
        );
        if stop {
            assert_eq!(w.device.state.lock().unwrap().loads, 0);
        } else {
            assert_eq!(w.playing().await.state, PlaybackStatus::Playing);
        }
    }
}
#[tokio::test]
async fn interrupted_preparation_is_read_only_on_reopen_and_cancelled_ipc_cannot_play() {
    let w = World::new(plan("song"));
    let revision = w.revision().await;
    let id = id();
    w.store
        .put(
            "9",
            "relay-playback-intents-v1",
            &id,
            &json!({"id":id,"pull_id":id,"previous_id":null,"phase":"preparing","created_at":1}),
        )
        .unwrap();
    let view = w.client.relay_view(&w.epoch(), false).await.unwrap().value;
    assert_eq!(view.operations[0].phase, RelayPlaybackPhase::Uncertain);
    assert!(!view.operations[0].can_retry);
    assert_eq!(w.api.pulls(), 0);
    let cancelled = uuid::Uuid::new_v4().to_string();
    w.client
        .relay_command(
            &w.epoch(),
            RelayCommand::CancelUnseen {
                id: cancelled.clone(),
                revision: revision.clone(),
            },
        )
        .await
        .unwrap();
    w.play(&cancelled, &revision).await;
    assert_eq!(w.api.pulls(), 0);
    assert_eq!(w.device.state.lock().unwrap().loads, 0);
}
#[tokio::test]
async fn account_switch_during_metadata_keeps_original_journal_and_never_installs_old_queue() {
    let w = World::new(plan("song"));
    let revision = w.revision().await;
    w.api.hold.store(true, Ordering::SeqCst);
    let client = w.client.clone();
    let epoch = w.epoch();
    let id = id();
    let copy = id.clone();
    let task = tokio::spawn(async move {
        client
            .relay_command(
                &epoch,
                RelayCommand::Continue {
                    id: copy,
                    revision,
                    quality: Quality::Standard,
                },
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), w.api.started.notified())
        .await
        .unwrap();
    w.session
        .adopt(
            w.api.clone(),
            json!({"userId":10}),
            &w.session.token().unwrap(),
        )
        .unwrap();
    w.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        w.store
            .get("9", "relay-playback-intents-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "failed"
    );
    assert!(
        w.client
            .relay_view(&w.epoch(), false)
            .await
            .unwrap()
            .value
            .operations
            .is_empty()
    );
    assert_eq!(w.device.state.lock().unwrap().loads, 0);
    assert!(
        RelayService::new(w.session.clone())
            .unwrap()
            .intent_for(&w.session.token().unwrap(), &id)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn completed_intent_identity_and_bounded_history_never_repeat_playback() {
    let w = World::new(plan("song"));
    let revision = w.revision().await;
    let first = id();
    w.play(&first, &revision).await;
    w.playing().await;
    let before = w.device.state.lock().unwrap().loads;
    for (other_revision, quality) in [
        ("0".repeat(64), Quality::Standard),
        (revision.clone(), Quality::Lossless),
    ] {
        assert!(matches!(
            w.client
                .relay_command(
                    &w.epoch(),
                    RelayCommand::Continue {
                        id: first.clone(),
                        revision: other_revision,
                        quality,
                    }
                )
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    for _ in 0..25 {
        let newer = id();
        w.store.put("9", "relay-playback-intents-v1", &newer,
            &json!({"id":newer,"pull_id":newer,"previous_id":null,"phase":"cancelled","created_at":u64::MAX})).unwrap();
    }
    assert!(
        !w.client
            .relay_view(&w.epoch(), false)
            .await
            .unwrap()
            .value
            .operations
            .iter()
            .any(|r| r.id == first)
    );
    let page = w.play(&first, &revision).await;
    assert_eq!(page.operations.len(), 20);
    assert!(
        page.operations
            .iter()
            .any(|r| r.id == first && r.phase == RelayPlaybackPhase::Installed)
    );
    assert_eq!(w.api.pulls(), 1);
    assert_eq!(w.device.state.lock().unwrap().loads, before);
}

#[tokio::test]
async fn installed_queue_marker_recovers_application_without_loading_it_again() {
    let w = World::new(plan("voice"));
    let revision = w.revision().await;
    let application = id();
    w.play(&application, &revision).await;
    w.playing().await;
    let before = w.device.state.lock().unwrap().loads;
    let mut record = w
        .store
        .get("9", "relay-playback-intents-v1", &application)
        .unwrap()
        .unwrap();
    record["phase"] = json!("applying");
    w.store
        .put("9", "relay-playback-intents-v1", &application, &record)
        .unwrap();
    let page = w.client.relay_view(&w.epoch(), false).await.unwrap().value;
    assert_eq!(page.operations[0].phase, RelayPlaybackPhase::Installed);
    assert_eq!(w.device.state.lock().unwrap().loads, before);
    assert_eq!(w.api.pulls(), 1);
}
