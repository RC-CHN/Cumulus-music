use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const ALLOWED: &str = "/api/w/v1/user/detail/73";
const BLOCKABLE: &str = "/api/w/v1/user/detail/74";
const NOTES: &str = "/api/event/get/73";
const BLOCKED_NOTES: &str = "/api/event/get/74";
const DETAIL: &str = "/api/v1/event/73/31";
const COMMENTS: &str = "/api/v1/resource/comments/A_EV_2_31_73";
const CURSOR: u64 = 1_700_000_010_321;

#[derive(Default)]
pub struct Library {
    blocked: AtomicBool,
    calls: Mutex<Vec<(String, Value)>>,
}

impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_PROFILE_NOTES_PROOF").as_deref() == Ok("1")
    }

    pub fn own_enabled() -> bool {
        std::env::var("CUMULUS_OWN_PROFILE_PROOF").as_deref() == Ok("1")
    }

    fn own_request(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        let account = || json!({"userId":2,"nickname":"合成本人资料","eventCount":3,"follows":4,"followeds":5});
        let value = match path {
            "/api/w/nuser/account/get" if *payload == json!({}) => {
                json!({"code":200,"profile":account()})
            }
            "/api/w/v1/user/detail/2" if *payload == json!({"all":true,"userId":"2"}) => {
                json!({"code":200,"profile":account(),"listenSongs":51})
            }
            "/api/event/get/2"
                if *payload == json!({"offset":0,"limit":20,"getcounts":true,"time":-1}) =>
            {
                json!({"code":200,"events":[row(51,2,"合成本人笔记")],"more":false})
            }
            "/api/user/follow/users/mixed/get/v2"
                if *payload
                    == json!({
                        "page":{"size":19,"cursor":"0"},"size":19,"scene":0,"sortType":0,"authority":false
                    }) =>
            {
                json!({"code":200,"data":{"records":[],"page":{"more":false,"recordCount":0}}})
            }
            "/api/user/getfolloweds/"
                if *payload
                    == json!({"userId":"2","time":0,"offset":0,"limit":20,"getcounts":true}) =>
            {
                json!({"code":200,"followeds":[],"more":false,"size":0})
            }
            _ => return None,
        };
        assert!(!cache);
        self.calls
            .lock()
            .unwrap()
            .push((path.into(), payload.clone()));
        Some(Ok(value))
    }

    pub fn block(&self) {
        self.blocked.store(true, Ordering::SeqCst);
    }

    pub fn report(&self) -> Value {
        json!(*self.calls.lock().unwrap())
    }

    pub fn request(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        if Self::own_enabled() {
            return self.own_request(path, payload, cache);
        }
        if !Self::enabled() {
            return None;
        }
        if path == "/api/v6/playlist/detail" {
            let (id, uid) = match payload["id"].as_str() {
                Some("20") => (20, 73),
                Some("21") => (21, 74),
                _ => return None,
            };
            return Some(Ok(json!({"code":200,"playlist":{
                "id":id,"userId":uid,"name":format!("合成笔记歌单 {id}"),
                "creator":{"userId":uid,"nickname":author(uid)},"trackCount":0,
                "privacy":0,"trackIds":[],"tracks":[]}})));
        }
        if ![ALLOWED, BLOCKABLE, NOTES, BLOCKED_NOTES, DETAIL, COMMENTS].contains(&path) {
            return None;
        }
        let attempt = {
            let mut calls = self.calls.lock().unwrap();
            calls.push((path.into(), payload.clone()));
            calls
                .iter()
                .filter(|(p, body)| p == path && body == payload)
                .count()
        };
        assert!(!cache, "profile notes proof used a cached request");
        Some(Ok(match path {
            ALLOWED | BLOCKABLE => {
                let uid = if path == ALLOWED { 73 } else { 74 };
                assert_eq!(*payload, json!({"all":true,"userId":uid.to_string()}));
                json!({"code":200,"profile":{"userId":uid,"nickname":author(uid),
                    "eventCount":17,"inBlacklist":uid == 74 && self.blocked.load(Ordering::SeqCst)}})
            }
            NOTES => {
                let time = &payload["time"];
                assert_eq!(
                    *payload,
                    json!({"offset":0,"limit":20,"getcounts":true,"time":time})
                );
                if time == -1 {
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        let mut events = (31..=42)
                            .map(|id| row(id, 73, &format!("合成笔记 {id}")))
                            .collect::<Vec<_>>();
                        events.extend(rejected_rows());
                        json!({"code":200,"events":events,"more":true,"lasttime":CURSOR.to_string()})
                    }
                } else {
                    assert_eq!(*time, CURSOR);
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        json!({"code":200,"events":[row(43,73,"合成续页笔记 43"),
                            row(82,999,"MISMATCH_PRIVATE")],"more":false})
                    }
                }
            }
            DETAIL if *payload == json!({}) => {
                json!({"code":200,"event":row(31,73,"合成笔记详情")})
            }
            COMMENTS => {
                assert_eq!(
                    *payload,
                    json!({"limit":60,"offset":0,"beforeTime":0,
                    "compareUserLocation":false,"commentId":"0","composeConcert":false,
                    "markReplied":false,"forceFlatComment":false,"showInner":false})
                );
                json!({"code":200,"total":1,"more":false,"comments":[{
                    "commentId":101,"likedCount":0,"content":"合成笔记评论","time":1700000000001_u64,
                    "user":{"userId":91,"nickname":"合成笔记评论作者"}}]})
            }
            _ => {
                return Some(Err(Error::Response(
                    "unexpected profile notes fixture request",
                )));
            }
        }))
    }
}

fn author(uid: u64) -> String {
    format!("合成笔记作者 {uid}")
}

fn row(id: u64, uid: u64, label: &str) -> Value {
    json!({"id":id,"type":19,"eventTime":1700000000000_u64-id,"user":{
        "userId":uid,"nickname":author(uid),"signature":"PAYLOAD_PRIVATE"
    },"info":{"commentCount":u64::from(id == 31),"threadId":format!("A_EV_2_{id}_{uid}")},
    "json":json!({"title":label,"msg":"此内容为合成笔记。"}).to_string()})
}

fn rejected_rows() -> [Value; 3] {
    let mut anonymous = row(80, 73, "ANONYMOUS_PRIVATE");
    anonymous["anonymityInfo"] = json!({"encryptUserId":"PAYLOAD_PRIVATE"});
    let mut missing = row(81, 73, "OWNER_MISSING_PRIVATE");
    missing["user"] = json!({"nickname":"OWNER_MISSING_PRIVATE"});
    [row(79, 999, "MISMATCH_PRIVATE"), anonymous, missing]
}
