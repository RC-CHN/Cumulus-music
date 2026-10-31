#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
#[path = "support/together_im.rs"]
pub(crate) mod im;
#[path = "support/together_incoming.rs"]
mod incoming_tests;
#[path = "support/together_lifecycle_host.rs"]
mod lifecycle_tests;
#[path = "support/together_mini.rs"]
mod mini_tests;
#[path = "support/together_progress.rs"]
mod progress_tests;
#[path = "support/together_shared.rs"]
mod shared_tests;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    playback::PlaybackMode,
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
#[derive(Default)]
pub(crate) struct RoomState {
    pub(crate) inside: bool,
    pub(crate) peer: bool,
    pub(crate) playlist: Option<Value>,
    pub(crate) play: Option<Value>,
    pub(crate) replacement_room: Option<String>,
}
#[derive(Default)]
pub(crate) struct Backend {
    pub(crate) state: Mutex<RoomState>,
    pub(crate) calls: Mutex<Vec<(String, Value)>>,
    drop_play: AtomicBool,
    pub(crate) drop_end: AtomicBool,
    pub(crate) common: Mutex<Option<Vec<Value>>>,
    pub(crate) shell: AtomicBool,
    hold_privilege: AtomicBool,
    privilege_started: tokio::sync::Notify,
    privilege_release: tokio::sync::Notify,
}
fn room() -> Value {
    json!({"roomId":"test-room","creatorId":1,"chatRoomId":"31","agoraChannelId":"private-channel","roomUsers":[{"userId":1,"nickname":"Listener","avatarUrl":"https://p1.music.126.net/avatar?token=SECRET"}]})
}
fn observed_room(state: &RoomState) -> Value {
    let mut value = room();
    if let Some(id) = &state.replacement_room {
        value["roomId"] = json!(id);
    }
    if state.peer {
        value["roomUsers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"userId":2,"nickname":"Peer"}));
    }
    value
}
fn song(id: &Value) -> Value {
    json!({"id":id,"name":format!("Song {id}"),"ar":[{"name":"Artist"}],"al":{"name":"Album","picUrl":"https://p1.music.126.net/cover?token=SECRET"},"dt":120000,"fee":0,"cookie":"SECRET"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            if path == "/api/listen/together/privilege/get"
                && self.hold_privilege.load(Ordering::SeqCst)
            {
                self.privilege_started.notify_one();
                self.privilege_release.notified().await;
            }
            let mut state = self.state.lock().unwrap();
            Ok(match path {
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> = serde_json::from_str(p["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|v|{let mut row=song(&v["id"]);if self.shell.load(Ordering::SeqCst){row["al"]["picUrl"]=Value::Null;}row}).collect::<Vec<_>>(),"privileges":[]})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/native.mp3?secret=SECRET","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/listen/together/status/get" => {
                    json!({"code":200,"data":{"inRoom":state.inside,"roomInfo":if state.inside {observed_room(&state)} else {Value::Null}}})
                }
                "/api/listen/together/room/create" => {
                    assert!(!state.inside);
                    state.inside = true;
                    json!({"code":200,"data":{"type":"NEW_ROOM","roomInfo":room()}})
                }
                "/api/listen/together/sync/playlist/get" => {
                    json!({"code":200,"data":{"playlist":state.playlist,"playCommand":state.play}})
                }
                "/api/listen/together/sync/list/command/report" => {
                    let c: Value =
                        serde_json::from_str(p["playlistParam"].as_str().unwrap()).unwrap();
                    assert_eq!(c["commandType"], "REPLACE");
                    state.playlist = Some(
                        json!({"displayList":{"result":c["displayList"],"changed":true},"randomList":if c["playMode"]=="RANDOM" {json!({"result":c["randomList"],"changed":true})} else {Value::Null},"playMode":c["playMode"],"replace":true,"version":c["version"]}),
                    );
                    if state.play.as_ref().is_none_or(|v| {
                        !c["displayList"]
                            .as_array()
                            .unwrap()
                            .contains(&v["targetSongId"])
                    }) {
                        state.play = Some(
                            json!({"commandType":"PLAY","progress":0,"playStatus":"PLAY","formerSongId":c["displayList"][0],"targetSongId":c["displayList"][0],"clientSeq":0,"serverSeq":0,"userId":1}),
                        );
                    }
                    json!({"code":200,"data":{"result":true}})
                }
                "/api/listen/together/play/command/report" => {
                    let mut c: Value =
                        serde_json::from_str(p["commandInfo"].as_str().unwrap()).unwrap();
                    c["userId"] = json!(1);
                    c["serverSeq"] = c["clientSeq"].clone();
                    state.play = Some(c);
                    if self.drop_play.swap(false, Ordering::SeqCst) {
                        return Err(Error::Transport("lost acknowledgement".into()));
                    }
                    json!({"code":200,"data":{"result":true}})
                }
                "/api/listen/together/end/v2" => {
                    state.inside = false;
                    state.peer = false;
                    state.playlist = None;
                    state.play = None;
                    if self.drop_end.swap(false, Ordering::SeqCst) {
                        return Err(Error::Transport("lost end acknowledgement".into()));
                    }
                    json!({"code":200,"data":{"success":true}})
                }
                "/api/listen/together/heartbeat" => json!({"code":200,"data":{"result":true}}),
                "/api/middle/im/token/get" => {
                    json!({"code":200,"data":{"uid":1,"accId":"test-account","token":"private-token"}})
                }
                "/api/listen/together/privilege/get" => {
                    assert_eq!(p["otherUserId"], "2");
                    let rows = self.common.lock().unwrap().clone().unwrap_or_else(|| p["songIds"].as_array().unwrap().iter()
                        .map(|id| json!({"songId":id,"playable":true,"unplayableType":"","unplayableUserIds":[],"unplayableOuterIds":[]})).collect());
                    json!({"code":200,"data":{"result":rows}})
                }
                "/api/w/nuser/account/get" if self.shell.load(Ordering::SeqCst) => {
                    json!({"code":200,"profile":{"userId":1,"nickname":"本地验证账号"}})
                }
                "/api/song/like/get" if self.shell.load(Ordering::SeqCst) => {
                    json!({"code":200,"ids":[]})
                }
                "/api/user/playlist" if self.shell.load(Ordering::SeqCst) => {
                    json!({"code":200,"playlist":[],"more":false})
                }
                "/api/v3/discovery/recommend/songs" if self.shell.load(Ordering::SeqCst) => {
                    json!({"code":200,"data":{"dailySongs":[]}})
                }
                _ if self.shell.load(Ordering::SeqCst) && path.contains("lyric") => {
                    json!({"code":200,"lrc":{"lyric":""}})
                }
                _ if self.shell.load(Ordering::SeqCst) => {
                    return Err(Error::Response("unsupported local desktop fixture read"));
                }
                _ => panic!("unexpected request {path}"),
            })
        })
    }
}

#[tokio::test]
async fn message_connection_reconnects_without_replaying_playback_and_exit_awaits_cleanup() {
    use cumulus_core::listen_together::transport::ConnectionState;
    use std::sync::atomic::Ordering;
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let device = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(device.clone())));
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let ready = wait(&client, |v| {
        v.connection.state == ConnectionState::Connected
            && v.connection.received == 1
            && v.tracks.len() == 3
            && v.target_id.is_some()
    })
    .await;
    let id = ready.room.unwrap().id;
    let write_count = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("command/report"))
            .count()
    };
    let before = write_count();
    client
        .together_command(
            "1",
            TogetherCommand::Reconnect {
                room_id: id.clone(),
            },
        )
        .await
        .unwrap();
    wait(&client, |v| {
        v.connection.state == ConnectionState::Connected && v.connection.received == 1
    })
    .await;
    assert_eq!(device.peak.load(Ordering::SeqCst), 1);
    assert_eq!(device.calls.load(Ordering::SeqCst), 2);
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
    assert_eq!(
        write_count(),
        before,
        "reconnect must not replay room/player writes"
    );
    let body = serde_json::to_string(&client.together_view("1", false).await.unwrap()).unwrap();
    assert!(
        !body.contains("private-token")
            && !body.contains("test-account")
            && !body.contains("never serialize")
    );
    client
        .together_command("1", TogetherCommand::End { room_id: id })
        .await
        .unwrap();
    assert_eq!(device.active.load(Ordering::SeqCst), 0);
    assert_eq!(device.closed.load(Ordering::SeqCst), 2);
    assert!(!client.has_together_cleanup().unwrap());
    assert_eq!(session.token().unwrap().generation, 1);
}

#[tokio::test]
async fn failed_native_cleanup_remains_visible_and_cannot_be_claimed_clean_on_second_close() {
    use cumulus_core::listen_together::transport::ConnectionState;
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let device = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(device.clone())));
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let ready = wait(&client, |v| {
        v.connection.state == ConnectionState::Connected && v.tracks.len() == 3
    })
    .await;
    device.close_failure.store(true, Ordering::SeqCst);
    assert!(
        client
            .together_command(
                "1",
                TogetherCommand::End {
                    room_id: ready.room.unwrap().id
                }
            )
            .await
            .is_err()
    );
    let view = client.together_view("1", false).await.unwrap().value;
    assert!(view.room.is_none() && view.local_recovery);
    assert_eq!(view.connection.state, ConnectionState::Failed);
    assert!(client.close_together().await.is_err());
    assert!(client.has_together_cleanup().unwrap());
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("end/v2"))
            .count(),
        1
    );
}
pub(crate) fn world(
    path: &std::path::Path,
    api: Arc<Backend>,
) -> (Arc<AccountSession>, MusicClient) {
    let registry = ProfileRegistry::open(path, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    (session, client)
}
async fn wait(client: &MusicClient, predicate: impl Fn(&TogetherView) -> bool) -> TogetherView {
    let mut last = None;
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let v = client.together_view("1", false).await.unwrap().value;
            if predicate(&v) {
                return v;
            }
            last = Some(v);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!(
            "together native state did not settle; last public view: {}",
            serde_json::to_string(&last).unwrap()
        )
    })
}
async fn play(client: &MusicClient) {
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["91".into(), "92".into(), "93".into()],
                index: 0,
                quality: cumulus_core::membership::Quality::Standard,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn actual_native_player_drives_room_queue_seek_pause_next_remove_and_restores_preferences() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.player_action("1", PlayerAction::Speed { value: 1.25 })
        .await
        .unwrap();
    c.player_action(
        "1",
        PlayerAction::Mode {
            mode: PlaybackMode::Sequential,
        },
    )
    .await
    .unwrap();
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let v = wait(&c, |v| {
        v.tracks.len() == 3 && v.pending.is_empty() && v.target_id.as_deref() == Some("91")
    })
    .await;
    assert!(v.attached);
    assert!(matches!(
        c.player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential
            }
        )
        .await,
        Err(Error::State(
            "together does not support sequential playback"
        ))
    ));
    assert!(!v.realtime_ready);
    assert!(c.has_together_cleanup().unwrap());
    assert!(matches!(
        c.begin_qr().await,
        Err(Error::State("leave together before switching accounts"))
    ));
    assert_eq!(v.tracks[1].name, "Song \"92\"");
    assert!(!serde_json::to_string(&v).unwrap().contains("SECRET"));
    assert!(
        !serde_json::to_string(&v)
            .unwrap()
            .contains("private-channel")
    );
    assert!(
        c.player_action("1", PlayerAction::Speed { value: 1.5 })
            .await
            .is_err()
    );
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&c, |v| !v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    wait(&c, |v| v.progress_ms == Some(12000) && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Next).await.unwrap();
    wait(&c, |v| {
        v.target_id.as_deref() == Some("92") && v.pending.is_empty()
    })
    .await;
    c.together_command(
        "1",
        TogetherCommand::Remove {
            room_id: "test-room".into(),
            song_id: "93".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| v.tracks.len() == 2 && v.pending.is_empty()).await;
    c.together_command(
        "1",
        TogetherCommand::End {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    let mut subscription = c.subscribe_playback("1").await.unwrap();
    let current = subscription.current().unwrap();
    assert_eq!(current.speed, 1.25);
    assert_eq!(current.mode, PlaybackMode::Sequential);
    assert_eq!(c.player_queue("1").await.unwrap().tracks.len(), 2);
    assert!(
        c.together_view("1", false)
            .await
            .unwrap()
            .value
            .room
            .is_none()
    );
    assert!(!api.state.lock().unwrap().inside);
    assert!(!c.has_together_cleanup().unwrap());
    let _ = &mut subscription;
    let commands = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .map(|(_, v)| serde_json::from_str::<Value>(v["commandInfo"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(commands.iter().any(|v| v["commandType"] == "NEXT"));
    assert!(
        commands
            .iter()
            .any(|v| v["commandType"] == "PROGRESS" && v["progress"] == 12000)
    );
}
#[tokio::test]
async fn lost_report_blocks_resending_but_native_audio_remains_controllable_and_can_resume() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| {
        v.tracks.len() == 3 && v.target_id.is_some() && v.pending.is_empty()
    })
    .await;
    api.drop_play.store(true, Ordering::SeqCst);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let v = wait(&c, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    assert_eq!(v.pending.len(), 1);
    let before = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .count();
    c.player_action("1", PlayerAction::Seek { seconds: 15.0 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Play).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        before,
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("play/command/report"))
            .count()
    );
    c.together_command(
        "1",
        TogetherCommand::Reconcile {
            operation_id: v.pending[0].id.clone(),
        },
    )
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Publish {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| {
        v.playing && v.progress_ms == Some(15000) && v.pending.is_empty() && v.sync_error.is_none()
    })
    .await;
    // Local selection may change while room writes are suspended. Resuming
    // must select that song remotely, not send a cross-song PROGRESS command.
    api.drop_play.store(true, Ordering::SeqCst);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let pending = wait(&c, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    c.player_action("1", PlayerAction::Next).await.unwrap();
    let subscription = c.subscribe_playback("1").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = subscription.current().unwrap();
            if snapshot.state == cumulus_core::playback::controller::PlaybackStatus::Playing
                && snapshot.track.as_ref().is_some_and(|v| v.id == "92")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Reconcile {
            operation_id: pending.pending[0].id.clone(),
        },
    )
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Publish {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| {
        v.target_id.as_deref() == Some("92") && v.playing && v.pending.is_empty()
    })
    .await;
    assert_eq!(
        api.state.lock().unwrap().play.as_ref().unwrap()["commandType"],
        "GOTO"
    );
    c.close_together().await.unwrap();
    assert!(!api.state.lock().unwrap().inside);
}
#[tokio::test]
async fn explicit_seek_to_zero_while_playing_is_reported_once() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&c, |v| !v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    wait(&c, |v| v.progress_ms == Some(12000) && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Play).await.unwrap();
    wait(&c, |v| v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 0.0 })
        .await
        .unwrap();
    wait(&c, |v| {
        v.playing && v.progress_ms == Some(0) && v.pending.is_empty()
    })
    .await;
    let reports = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .map(|(_, v)| serde_json::from_str::<Value>(v["commandInfo"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        reports
            .iter()
            .filter(|v| v["commandType"] == "GOTO" && v["progress"] == 0)
            .count(),
        1
    );
    c.close_together().await.unwrap();
}
#[tokio::test]
async fn reopening_requires_explicit_attach_and_account_switch_never_exposes_old_room() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    drop(c);
    drop(s);
    let (s, c) = world(root.path(), api.clone());
    let before = api.calls.lock().unwrap().len();
    assert!(c.has_together_cleanup().unwrap());
    let view = c.together_view("1", false).await.unwrap().value;
    assert!(view.room.is_some());
    assert!(!view.attached);
    assert_eq!(before, api.calls.lock().unwrap().len());
    c.together_command(
        "1",
        TogetherCommand::Attach {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    assert!(c.together_view("1", false).await.unwrap().value.attached);
    s.adopt(api.clone(), json!({"userId":2}), &s.token().unwrap())
        .unwrap();
    let new = c.together_view("2", false).await.unwrap().value;
    assert!(new.room.is_none());
    assert!(!new.attached);
    assert!(new.pending.is_empty());
    assert!(new.sync_error.is_none());
    assert!(matches!(
        c.together_command("1", TogetherCommand::Create).await,
        Err(Error::StaleResponse)
    ));
}
