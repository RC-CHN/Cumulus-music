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
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    hold_directory: AtomicBool,
    fail_directory: AtomicBool,
    top_mode: AtomicU8,
    credential_revision: AtomicU64,
    deny: AtomicBool,
    started: Notify,
    release: Notify,
}
fn voice(id: u64) -> Value {
    json!({"id":id,"name":format!("Episode {id}"),"mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":{"id":7,"name":"Radio"},"dj":{"nickname":"Host"},"cookie":"SECRET"})
}
impl Api for Backend {
    fn verified_pc_experiment_revision(&self) -> cumulus_core::Result<Option<String>> {
        let current = self.credential_revision.load(Ordering::SeqCst);
        Ok((current != u64::MAX).then(|| current.to_string()))
    }
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => {
                    assert_eq!(p, json!({"expNames":"podcast_tab_better"}));
                    let group =
                        |group| json!({"expName":"podcast_tab_better","expGroupName":group});
                    let rows = match self.top_mode.load(Ordering::SeqCst) {
                        0 => vec![group("c")],
                        1 => vec![group("t1")],
                        2 => {
                            return Err(cumulus_core::Error::Transport(
                                "sealed assignment lost".into(),
                            ));
                        }
                        3 => vec![group("c"), group("t1")],
                        4 => vec![group("")],
                        _ => panic!("unexpected fixture mode"),
                    };
                    json!({"code":200,"data":rows})
                }
                "/api/voice/homepage/block/content" => {
                    assert_eq!(p, json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"}));
                    if self.fail_directory.load(Ordering::SeqCst) {
                        return Err(cumulus_core::Error::Transport(
                            "sealed directory lost".into(),
                        ));
                    }
                    if self.hold_directory.swap(false, Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
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
fn auto(revision: &str, visit: &str) -> SceneFmCommand {
    SceneFmCommand::HomeAutomatic {
        revision: revision.into(),
        visit: visit.into(),
    }
}
async fn top(c: &MusicClient) -> Value {
    serde_json::to_value(
        c.podcast_rankings("1", PodcastRankQuery::TopConfig)
            .await
            .unwrap()
            .value,
    )
    .unwrap()
}
async fn enter(c: &MusicClient) -> String {
    assert_eq!(
        top(c).await,
        json!({"kind":"top_config","layout":"classic","sceneFmAllowed":true})
    );
    c.scene_fm_home_enter("1")
        .await
        .unwrap()
        .value
        .unwrap()
        .visit
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
    let (_dir, api, _s, c, device) = fixture();
    assert!(!c.scene_fm_autoplay_preference("1").unwrap().value);
    let off = enter(&c).await;
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    let r = v.revision.unwrap();
    c.scene_fm_command("1", auto(&r, &off)).await.unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 0);
    let before = api.calls.lock().unwrap().len();
    assert!(c.set_scene_fm_autoplay_preference("1", true).unwrap().value);
    assert_eq!(api.calls.lock().unwrap().len(), before);
    assert_eq!(device.state.lock().unwrap().loads, 0);
    let first = enter(&c).await;
    c.scene_fm_home_leave("1", &off).unwrap(); // Late leave cannot revoke first.
    let v = c
        .scene_fm_command("1", auto(&r, &first))
        .await
        .unwrap()
        .value;
    assert!(v.playing);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let second = enter(&c).await;
    c.scene_fm_command("1", auto(v.revision.as_deref().unwrap(), &second))
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
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    let entry = enter(&c).await;
    c.scene_fm_command("1", auto(v.revision.as_deref().unwrap(), &entry))
        .await
        .unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn playing_entry_does_not_consume_automatic_for_a_later_paused_entry() {
    let (_dir, _api, _s, c, device) = fixture();
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    c.play_selection("1", PlaySelection::Voice { id: "1".into() })
        .await
        .unwrap();
    let playing = enter(&c).await;
    let revision = c
        .scene_fm_view("1", true)
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    c.scene_fm_command("1", auto(&revision, &playing))
        .await
        .unwrap();
    assert_eq!(device.state.lock().unwrap().loads, 1);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = enter(&c).await;
    assert!(
        c.scene_fm_command("1", auto(&revision, &paused))
            .await
            .unwrap()
            .value
            .playing
    );
    assert_eq!(device.state.lock().unwrap().loads, 2);
}

#[tokio::test]
async fn autoplay_preference_reopens_locally_and_is_account_isolated() {
    let (dir, api, session, c, _device) = fixture();
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    assert!(api.calls.lock().unwrap().is_empty());
    drop(c);
    let reopened = MusicClient::with_session(
        session.clone(),
        ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap(),
    );
    assert!(reopened.scene_fm_autoplay_preference("1").unwrap().value);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(reopened.scene_fm_autoplay_preference("1").is_err());
    assert!(!reopened.scene_fm_autoplay_preference("2").unwrap().value);
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(reopened.scene_fm_autoplay_preference("3").unwrap().value);
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn delayed_automatic_preparation_cannot_undo_a_newer_stop_or_leave() {
    for leave in [false, true] {
        let (_dir, api, _s, c, device) = fixture();
        api.hold.store(true, Ordering::SeqCst);
        c.set_scene_fm_autoplay_preference("1", true).unwrap();
        let visit = enter(&c).await;
        let v = c.scene_fm_view("1", true).await.unwrap().value;
        let task = {
            let (c, visit) = (c.clone(), visit.clone());
            tokio::spawn(async move {
                c.scene_fm_command("1", auto(v.revision.as_deref().unwrap(), &visit))
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(3), api.started.notified())
            .await
            .unwrap();
        if leave {
            c.scene_fm_home_leave("1", &visit).unwrap();
        } else {
            c.player_action("1", PlayerAction::Stop).await.unwrap();
        }
        api.release.notify_one();
        assert!(task.await.unwrap().is_err());
        assert_eq!(device.state.lock().unwrap().loads, 0);
        assert!(c.player_queue("1").await.unwrap().tracks.is_empty());
        assert!(!c.scene_fm_view("1", false).await.unwrap().value.pending);
    }
}

#[tokio::test]
async fn verified_classic_home_is_the_only_automatic_authority() {
    let (_dir, api, _s, c, device) = fixture();
    let revision = c
        .scene_fm_view("1", true)
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    assert!(
        serde_json::from_value::<SceneFmCommand>(json!({"kind":"automatic","revision":revision}))
            .is_err()
    );
    for (mode, layout) in [
        (1, "updated"),
        (2, "classic"),
        (3, "classic"),
        (4, "classic"),
    ] {
        api.top_mode.store(mode, Ordering::SeqCst);
        assert_eq!(
            top(&c).await,
            json!({"kind":"top_config","layout":layout,"sceneFmAllowed":false})
        );
        let before = api.calls.lock().unwrap().len();
        assert!(c.scene_fm_home_enter("1").await.unwrap().value.is_none());
        assert!(
            c.scene_fm_command("1", auto(&revision, "forged"))
                .await
                .is_err()
        );
        assert_eq!(api.calls.lock().unwrap().len(), before);
    }
    api.top_mode.store(0, Ordering::SeqCst);
    api.credential_revision.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(top(&c).await["sceneFmAllowed"], false);
    api.credential_revision.store(1, Ordering::SeqCst);
    let old = enter(&c).await;
    api.credential_revision.store(2, Ordering::SeqCst);
    let before = api.calls.lock().unwrap().len();
    assert!(
        c.scene_fm_command("1", auto(&revision, &old))
            .await
            .is_err()
    );
    assert_eq!(api.calls.lock().unwrap().len(), before);
    assert_eq!(device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn directory_read_does_not_steal_a_later_stop_or_consume_the_next_entry() {
    let (_dir, api, _s, c, device) = fixture();
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    let first = enter(&c).await;
    api.hold_directory.store(true, Ordering::SeqCst);
    let directory = {
        let c = c.clone();
        tokio::spawn(async move { c.scene_fm_view("1", true).await.unwrap().value })
    };
    tokio::time::timeout(Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    api.release.notify_one();
    let revision = directory.await.unwrap().revision.unwrap();
    assert!(
        !c.scene_fm_command("1", auto(&revision, &first))
            .await
            .unwrap()
            .value
            .playing
    );
    assert_eq!(device.state.lock().unwrap().loads, 0);
    let second = enter(&c).await;
    assert!(
        c.scene_fm_command("1", auto(&revision, &second))
            .await
            .unwrap()
            .value
            .playing
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn an_explicit_directory_retry_can_use_the_same_unconsumed_home_visit() {
    let (_dir, api, _s, c, device) = fixture();
    c.set_scene_fm_autoplay_preference("1", true).unwrap();
    let visit = enter(&c).await;
    api.fail_directory.store(true, Ordering::SeqCst);
    assert!(c.scene_fm_view("1", true).await.is_err());
    assert_eq!(device.state.lock().unwrap().loads, 0);
    api.fail_directory.store(false, Ordering::SeqCst);
    let revision = c
        .scene_fm_view("1", true)
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    assert!(
        c.scene_fm_command("1", auto(&revision, &visit))
            .await
            .unwrap()
            .value
            .playing
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
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

fn open_voice(revision: &str, voice_id: &str) -> SceneFmCommand {
    SceneFmCommand::OpenVoice {
        revision: revision.into(),
        voice_id: voice_id.into(),
    }
}

#[tokio::test]
async fn title_uses_native_current_voice_when_card_or_local_completion_is_stale() {
    let (dir, api, _session, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    c.scene_fm_command("1", open_voice(v.revision.as_deref().unwrap(), "1"))
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Seek { seconds: 18.0 })
        .await
        .unwrap();
    let sub = c.subscribe_playback("1").await.unwrap();
    let before = sub.current().unwrap();
    let queue = serde_json::to_value(c.player_queue("1").await.unwrap()).unwrap();
    let calls = api.calls.lock().unwrap().len();
    let native_revision = before.selected.as_ref().unwrap().source["scene_revision"]
        .as_u64()
        .unwrap();
    // Reproduce the legitimate interval after the exact prepared queue was
    // loaded but before a card view observes and confirms its pending record.
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let mut pending = store.get("9", "scene-fm-v1", "current").unwrap().unwrap();
    pending["revision"] = json!(native_revision);
    pending["pending"] = json!(true);
    store.put("9", "scene-fm-v1", "current", &pending).unwrap();
    let v = c
        .scene_fm_command("1", open_voice(&native_revision.to_string(), "1"))
        .await
        .unwrap()
        .value;
    assert!(v.active && v.playing && !v.pending);
    let after = sub.current().unwrap();
    assert_eq!(after.state, PlaybackStatus::Playing);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.control_revision, before.control_revision);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(after.position, Some(18.0));
    assert_eq!(
        serde_json::to_value(c.player_queue("1").await.unwrap()).unwrap(),
        queue
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(api.calls.lock().unwrap().len(), calls);

    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = sub.current().unwrap();
    let v = c
        .scene_fm_command("1", open_voice(v.revision.as_deref().unwrap(), "1"))
        .await
        .unwrap()
        .value;
    let resumed = sub.current().unwrap();
    assert!(v.active && v.playing);
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.queue_revision, paused.queue_revision);
    assert_eq!(resumed.position, Some(18.0));
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(device.state.lock().unwrap().pauses.last(), Some(&false));
    assert_eq!(
        serde_json::to_value(c.player_queue("1").await.unwrap()).unwrap(),
        queue
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);

    assert!(matches!(
        c.scene_fm_command("1", open_voice(v.revision.as_deref().unwrap(), "2"))
            .await,
        Err(cumulus_core::Error::QueueChanged)
    ));
    assert!(matches!(
        c.scene_fm_command("0", open_voice(v.revision.as_deref().unwrap(), "1"))
            .await,
        Err(cumulus_core::Error::StaleResponse)
    ));
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(
        serde_json::to_value(c.player_queue("1").await.unwrap()).unwrap(),
        queue
    );
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn title_starts_whole_scene_from_a_different_source_even_for_the_same_voice() {
    let (_dir, _api, _session, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    c.play_selection("1", PlaySelection::Voice { id: "1".into() })
        .await
        .unwrap();
    let before = c.subscribe_playback("1").await.unwrap().current().unwrap();
    assert_ne!(before.selected.as_ref().unwrap().source["kind"], "scene_fm");
    let v = c
        .scene_fm_command("1", open_voice(v.revision.as_deref().unwrap(), "1"))
        .await
        .unwrap()
        .value;
    let after = c.subscribe_playback("1").await.unwrap().current().unwrap();
    assert!(v.active && v.playing);
    assert_ne!(after.play_id, before.play_id);
    assert_eq!(after.selected.as_ref().unwrap().source["kind"], "scene_fm");
    assert_eq!(c.player_queue("1").await.unwrap().tracks.len(), 3);
    assert_eq!(device.state.lock().unwrap().loads, 2);
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn title_does_not_consume_an_unapplied_pending_revision_as_current() {
    let (dir, _api, _session, c, device) = fixture();
    let v = c.scene_fm_view("1", true).await.unwrap().value;
    let v = c
        .scene_fm_command("1", play(v.revision.as_deref().unwrap()))
        .await
        .unwrap()
        .value;
    let before = c.subscribe_playback("1").await.unwrap().current().unwrap();
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let mut pending = store.get("9", "scene-fm-v1", "current").unwrap().unwrap();
    let revision = v.revision.unwrap().parse::<u64>().unwrap() + 1;
    pending["revision"] = json!(revision);
    pending["pending"] = json!(true);
    store.put("9", "scene-fm-v1", "current", &pending).unwrap();
    let v = c
        .scene_fm_command("1", open_voice(&revision.to_string(), "1"))
        .await
        .unwrap()
        .value;
    let after = c.subscribe_playback("1").await.unwrap().current().unwrap();
    assert!(v.active && v.playing && !v.pending);
    assert_ne!(after.play_id, before.play_id);
    assert_ne!(after.queue_revision, before.queue_revision);
    assert_eq!(device.state.lock().unwrap().loads, 2);
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}
