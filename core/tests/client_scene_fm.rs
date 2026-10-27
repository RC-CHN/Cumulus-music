#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    playback::controller::PlaybackStatus,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    deny: AtomicBool,
    started: Notify,
    release: Notify,
}
fn voice(id: u64) -> Value {
    json!({"id":id,"name":format!("Episode {id}"),"mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":{"id":7,"name":"Radio"},"dj":{"nickname":"Host"},"cookie":"SECRET"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            Ok(match path {
                "/api/voice/homepage/block/content" => {
                    let mut v = voice(1);
                    if self.hold.load(Ordering::SeqCst) {
                        v.as_object_mut().unwrap().remove("mainSong");
                    }
                    json!({"code":200,"data":[{"creatives":[{"creativeId":"OP_1","creativeExtInfoVO":{"djPrograms":[v,voice(2),voice(3)]},"uiElement":{"mainTitle":{"title":"人间剧场"}}},{"creativeId":"OP_7","uiElement":{"mainTitle":{"title":"新闻资讯"}}}]}]})
                }
                "/api/content/scene/fm/play/list" => {
                    json!({"code":200,"data":{"list":[voice(4),voice(5)]}})
                }
                "/api/dj/program/detail" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"program":voice(p["id"].as_str().unwrap().parse().unwrap())})
                }
                "/api/dj/play/url/check" => {
                    let id = p["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":!self.deny.load(Ordering::SeqCst)}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/voice.mp3?key=SECRET","md5":"abc","expi":300,"level":"standard"}]})
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
    Arc<engine::Device>,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let device = Arc::new(engine::Device::default());
    let d = device.clone();
    let c = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(move || Ok(engine::audio_engine(d.clone())));
    (dir, api, session, Arc::new(c), device)
}
fn play(revision: &str) -> SceneFmCommand {
    SceneFmCommand::Play {
        revision: revision.into(),
    }
}
fn auto(revision: &str) -> SceneFmCommand {
    SceneFmCommand::Automatic {
        revision: revision.into(),
    }
}
#[tokio::test]
async fn whole_voice_queue_toggle_next_history_lazy_switch_and_restart() {
    let (dir, api, s, c, _device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    assert!(!serde_json::to_string(&v).unwrap().contains("SECRET"));
    assert!(!v.autoplay);
    let v = c
        .scene_fm_command("1", play(v.revision.as_deref().unwrap()))
        .await
        .unwrap()
        .value;
    assert!(v.active && v.playing && !v.pending);
    {
        let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
            .unwrap()
            .store();
        let lock =
            cumulus_core::storage::OperationLock::acquire(store.directory(), "scene-fm-v1", "9")
                .unwrap();
        assert!(c.scene_fm_view("1", false).await.unwrap().value.playing);
        drop(lock);
    }
    let queue = c.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 3);
    assert_eq!(queue.tracks[0].track.voice_id.as_deref(), Some("1"));
    assert!(!serde_json::to_string(&queue).unwrap().contains("SECRET"));
    let sub = c.subscribe_playback("1").await.unwrap();
    assert!(!sub.current().unwrap().fm);
    c.player_action("1", PlayerAction::Seek { seconds: 18.0 })
        .await
        .unwrap();
    let v = c
        .scene_fm_command(
            "1",
            SceneFmCommand::Toggle {
                revision: v.revision.unwrap(),
            },
        )
        .await
        .unwrap()
        .value;
    assert!(!v.playing);
    assert_eq!(sub.current().unwrap().position, Some(18.0));
    let v = c
        .scene_fm_command(
            "1",
            SceneFmCommand::Toggle {
                revision: v.revision.unwrap(),
            },
        )
        .await
        .unwrap()
        .value;
    assert!(v.playing);
    c.player_action("1", PlayerAction::Next).await.unwrap();
    let v = c.scene_fm_view("1", false).await.unwrap().value;
    assert_eq!(v.episode.unwrap().id, "2");
    let v = c
        .scene_fm_command(
            "1",
            SceneFmCommand::Select {
                revision: v.revision.unwrap(),
                scene_id: "OP_7".into(),
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(v.episode.unwrap().id, "4");
    assert_eq!(c.player_queue("1").await.unwrap().tracks.len(), 2);
    let v = c
        .scene_fm_command(
            "1",
            SceneFmCommand::Select {
                revision: v.revision.unwrap(),
                scene_id: "OP_1".into(),
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(v.episode.unwrap().id, "3");
    c.player_action("1", PlayerAction::Seek { seconds: 24.0 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = c.player_queue("1").await.unwrap();
    drop(sub);
    drop(c);
    let reopened = MusicClient::with_session(
        s.clone(),
        ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    let restored = reopened
        .player_action("1", PlayerAction::Restore)
        .await
        .unwrap();
    assert_eq!(restored.position, Some(24.0));
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(
        before
            .tracks
            .iter()
            .map(|e| &e.entry_id)
            .collect::<Vec<_>>(),
        reopened
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|e| &e.entry_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reopened
            .scene_fm_view("1", false)
            .await
            .unwrap()
            .value
            .episode
            .unwrap()
            .id,
        "3"
    );
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/api/content/scene/fm/play/list")
            .count(),
        1
    );
    reopened
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
}
#[tokio::test]
async fn autoplay_is_off_by_default_once_per_session_and_manual_play_consumes_it() {
    let (_dir, _api, _s, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    let r = v.revision.unwrap();
    c.scene_fm_command("1", auto(&r)).await.unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 0);
    c.scene_fm_command("1", SceneFmCommand::Autoplay { enabled: true })
        .await
        .unwrap();
    let v = c.scene_fm_command("1", auto(&r)).await.unwrap().value;
    assert!(v.playing);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    c.scene_fm_command("1", auto(v.revision.as_deref().unwrap()))
        .await
        .unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert!(!c.scene_fm_view("1", false).await.unwrap().value.playing);
    let (_dir, _api, _s, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    let v = c
        .scene_fm_command("1", play(v.revision.as_deref().unwrap()))
        .await
        .unwrap()
        .value;
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    c.scene_fm_command("1", SceneFmCommand::Autoplay { enabled: true })
        .await
        .unwrap();
    c.scene_fm_command("1", auto(v.revision.as_deref().unwrap()))
        .await
        .unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 1);
}
#[tokio::test]
async fn delayed_automatic_preparation_cannot_undo_a_newer_stop() {
    let (_dir, api, _s, c, device) = fixture();
    api.hold.store(true, Ordering::SeqCst);
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    c.scene_fm_command("1", SceneFmCommand::Autoplay { enabled: true })
        .await
        .unwrap();
    let task = {
        let c = c.clone();
        tokio::spawn(async move {
            c.scene_fm_command("1", auto(v.revision.as_deref().unwrap()))
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    api.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(device.state.lock().unwrap().loads, 0);
    assert!(c.player_queue("1").await.unwrap().tracks.is_empty());
    assert!(!c.scene_fm_view("1", false).await.unwrap().value.pending);
}
#[tokio::test]
async fn failed_authorization_retains_explicit_recovery_and_stale_account_has_no_requests() {
    let (_dir, api, s, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    api.deny.store(true, Ordering::SeqCst);
    assert!(
        c.scene_fm_command("1", play(v.revision.as_deref().unwrap()))
            .await
            .is_err()
    );
    let v = c.scene_fm_view("1", false).await.unwrap().value;
    assert!(v.pending);
    assert!(!v.playing);
    assert_eq!(device.state.lock().unwrap().loads, 0);
    api.deny.store(false, Ordering::SeqCst);
    let v = c
        .scene_fm_command("1", play(v.revision.as_deref().unwrap()))
        .await
        .unwrap()
        .value;
    assert!(v.playing && !v.pending);
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    let t = s.token().unwrap();
    s.adopt(api.clone(), json!({"userId":10}), &t).unwrap();
    let n = api.calls.lock().unwrap().len();
    assert!(
        c.scene_fm_command("1", play(v.revision.as_deref().unwrap()))
            .await
            .is_err()
    );
    assert_eq!(api.calls.lock().unwrap().len(), n);
    assert!(
        c.scene_fm_view("2", false)
            .await
            .unwrap()
            .value
            .revision
            .is_none()
    );
}
