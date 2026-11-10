//! Shared synthetic room fixtures for the core tests and desktop WebView probes.
#[allow(dead_code)]
#[path = "playback/mod.rs"]
pub(super) mod engine;
#[path = "together_im.rs"]
pub(crate) mod im;
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
    atomic::{AtomicBool, AtomicU16, Ordering},
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
    pub(crate) queue_rejection: AtomicU16,
    pub(crate) deny_queue_http: AtomicBool,
    pub(super) drop_play: AtomicBool,
    pub(crate) drop_end: AtomicBool,
    pub(crate) hold_end: AtomicBool,
    pub(crate) end_started: AtomicBool,
    pub(crate) end_release: tokio::sync::Notify,
    pub(crate) common: Mutex<Option<Vec<Value>>>,
    pub(crate) shell: AtomicBool,
    pub(crate) fail_statistics: AtomicBool,
    pub(crate) hold_statistics: AtomicBool,
    pub(crate) statistics_started: tokio::sync::Notify,
    pub(crate) statistics_release: tokio::sync::Notify,
    pub(super) hold_privilege: AtomicBool,
    pub(super) privilege_started: tokio::sync::Notify,
    pub(super) privilege_release: tokio::sync::Notify,
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
            if path == "/api/listen/together/end/v2" && self.hold_end.load(Ordering::SeqCst) {
                self.end_started.store(true, Ordering::SeqCst);
                self.end_release.notified().await;
            }
            if path == "/api/listen/together/privilege/get"
                && self.hold_privilege.load(Ordering::SeqCst)
            {
                self.privilege_started.notify_one();
                self.privilege_release.notified().await;
            }
            if path == "/api/listen/together/relation/statistics/get/v2"
                && self.hold_statistics.load(Ordering::SeqCst)
            {
                self.statistics_started.notify_one();
                self.statistics_release.notified().await;
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
                "/api/listen/together/relation/statistics/get/v2" => {
                    if self.fail_statistics.swap(false, Ordering::SeqCst) {
                        return Err(Error::Transport("synthetic statistics failure".into()));
                    }
                    assert_eq!(p["roomUserIds"], r#"["1","2"]"#);
                    json!({"code":200,"data":{"listenCount":12,"totalConnectionTime":3661,"privateExtra":"SECRET"}})
                }
                "/api/listen/together/sync/playlist/get" => {
                    json!({"code":200,"data":{"playlist":state.playlist,"playCommand":state.play}})
                }
                "/api/listen/together/sync/list/command/report" => {
                    if self.deny_queue_http.swap(false, Ordering::SeqCst) {
                        return Err(Error::HttpStatus {
                            status: 403,
                            endpoint: Some(path.into()),
                            body: Some(Box::new(json!({"code":488}))),
                        });
                    }
                    let code = self.queue_rejection.swap(0, Ordering::SeqCst);
                    if code != 0 {
                        return Ok(
                            json!({"code":code,"message":"never serialize service body","data":{"result":false}}),
                        );
                    }
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
                    assert_eq!(p["shareInfo"], r#"{"thisListenCount":1}"#);
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
