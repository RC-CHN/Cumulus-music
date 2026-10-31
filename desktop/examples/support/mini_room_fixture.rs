//! Offline single-member room protocol for the real two-window mini proof.
//! No HTTP transport, invitations, external IM or RTC connections are available.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
struct State {
    inside: bool,
    playlist: Option<Value>,
    play: Option<Value>,
    created: usize,
    ended: usize,
}
#[derive(Default)]
pub struct RoomFixture(Mutex<State>);
fn room() -> Value {
    json!({"roomId":"synthetic-mini-room","creatorId":2,"chatRoomId":"31","agoraChannelId":"synthetic-channel","roomUsers":[{"userId":2,"nickname":"Cumulus"}]})
}
impl RoomFixture {
    #[allow(dead_code)] // The shared library also serves the desktop-lyrics proof.
    pub fn report(&self) -> Value {
        let state = self.0.lock().unwrap();
        json!({"created":state.created,"ended":state.ended,"inside":state.inside,"realRequests":0})
    }
    pub fn response(&self, path: &str, args: &Value) -> Option<Result<Value>> {
        if !path.starts_with("/api/listen/together/") && path != "/api/middle/im/token/get" {
            return None;
        }
        Some((|| {
            let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
            Ok(match path {
                "/api/listen/together/status/get" => {
                    json!({"code":200,"data":{"inRoom":state.inside,"roomInfo":if state.inside {room()} else {Value::Null}}})
                }
                "/api/listen/together/room/create" => {
                    assert!(!state.inside);
                    state.inside = true;
                    state.created += 1;
                    json!({"code":200,"data":{"type":"NEW_ROOM","roomInfo":room()}})
                }
                "/api/listen/together/sync/playlist/get" => {
                    json!({"code":200,"data":{"playlist":state.playlist,"playCommand":state.play}})
                }
                "/api/listen/together/sync/list/command/report" => {
                    let command: Value =
                        serde_json::from_str(args["playlistParam"].as_str().unwrap())?;
                    assert_eq!(command["commandType"], "REPLACE");
                    state.playlist = Some(
                        json!({"displayList":{"result":command["displayList"],"changed":true},"randomList":if command["playMode"]=="RANDOM" {json!({"result":command["randomList"],"changed":true})} else {Value::Null},"playMode":command["playMode"],"replace":true,"version":command["version"]}),
                    );
                    if state.play.is_none() {
                        let id = command["displayList"][0].clone();
                        state.play = Some(
                            json!({"commandType":"PAUSE","progress":0,"playStatus":"PAUSE","formerSongId":id,"targetSongId":id,"clientSeq":0,"serverSeq":0,"userId":2}),
                        );
                    }
                    json!({"code":200,"data":{"result":true}})
                }
                "/api/listen/together/play/command/report" => {
                    let mut command: Value =
                        serde_json::from_str(args["commandInfo"].as_str().unwrap())?;
                    command["userId"] = json!(2);
                    command["serverSeq"] = command["clientSeq"].clone();
                    state.play = Some(command);
                    json!({"code":200,"data":{"result":true}})
                }
                "/api/listen/together/end/v2" => {
                    state.inside = false;
                    state.playlist = None;
                    state.play = None;
                    state.ended += 1;
                    json!({"code":200,"data":{"success":true}})
                }
                "/api/listen/together/heartbeat" => json!({"code":200,"data":{"result":true}}),
                "/api/middle/im/token/get" => {
                    json!({"code":200,"data":{"uid":2,"accId":"synthetic","token":"synthetic-no-connection"}})
                }
                _ => return Err(Error::Response("unsupported synthetic mini room endpoint")),
            })
        })())
    }
}
