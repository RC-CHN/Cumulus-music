use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const DETAIL: &str = "/api/w/v1/user/detail/";
const FOLLOW: &str = "/api/user/follow/73";
const UNFOLLOW: &str = "/api/user/delfollow/73";

#[derive(Default)]
struct State {
    calls: Vec<(String, Value)>,
    followed: bool,
    unfollow_sent: bool,
    old_read_seen: bool,
}

#[derive(Default)]
pub struct Library(Mutex<State>);

impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_PROFILE_FOLLOW_PROOF").as_deref() == Ok("1")
    }

    pub fn report(&self) -> Value {
        json!(&self.0.lock().unwrap().calls)
    }

    pub fn request(&self, path: &str, body: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() {
            return None;
        }
        if path == "/api/v6/playlist/detail" {
            let (id, uid) = match body["id"].as_str() {
                Some("31") => (31, 73),
                Some("32") => (32, 2),
                Some("33") => (33, 74),
                Some("34") => (34, 75),
                Some("35") => (35, 76),
                _ => return None,
            };
            return Some(Ok(json!({"code":200,"playlist":{
                "id":id,"userId":uid,"name":format!("合成关注歌单 {id}"),
                "creator":{"userId":uid,"nickname":author(uid)},"trackCount":0,
                "privacy":0,"trackIds":[],"tracks":[]}})));
        }
        let uid = path
            .strip_prefix(DETAIL)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|uid| [2, 73, 74, 75, 76].contains(uid));
        if uid.is_none()
            && !path.starts_with("/api/user/follow/")
            && !path.starts_with("/api/user/delfollow/")
        {
            return None;
        }
        assert!(!cache);
        let mut state = self.0.lock().unwrap();
        state.calls.push((path.into(), body.clone()));
        if let Some(uid) = uid {
            assert_eq!(*body, json!({"all":true,"userId":uid.to_string()}));
            if uid == 73 && state.unfollow_sent {
                if state.old_read_seen {
                    state.followed = false;
                } else {
                    state.old_read_seen = true;
                }
            }
            let count = if uid == 73 {
                if state.followed {
                    12
                } else if state.unfollow_sent {
                    8
                } else {
                    10
                }
            } else {
                0
            };
            return Some(Ok(
                json!({"code":200,"profile":{"userId":uid,"nickname":author(uid),
                "followed":uid == 73 && state.followed,"mutual":uid == 73 && state.followed,
                "followeds":count,"inBlacklist":uid==74,"blacklist":uid==76,
                "accountStatus":if uid==75 {40} else {0}}}),
            ));
        }
        assert_eq!(*body, json!({}));
        Some(match path {
            FOLLOW if !state.followed && !state.unfollow_sent => {
                state.followed = true;
                Ok(json!({"code":201}))
            }
            UNFOLLOW if state.followed && !state.unfollow_sent => {
                state.unfollow_sent = true;
                Err(Error::Transport("synthetic unfollow reply lost".into()))
            }
            _ => Err(Error::Response("unexpected synthetic profile mutation")),
        })
    }
}

fn author(uid: u32) -> String {
    format!("合成关注用户 {uid}")
}
