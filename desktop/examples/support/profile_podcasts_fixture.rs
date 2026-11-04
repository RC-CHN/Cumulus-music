use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const COUNT: &str = "/api/djradio/get/create/sub/count";
const CREATED: &str = "/api/djradio/get/byuser/v1";
const COLLECTED: &str = "/api/djradio/get/subed";
const DETAIL: &str = "/api/djradio/v3/get";
const PROGRAMS: &str = "/api/v6/dj/program/byradio";
const RESUME: &str = "/api/dj/user/setting/resumeplay/get";
const LIKED: &str = "/api/content/my/liked/voice";
const SSO: &str = "/api/sso/login/token";

#[derive(Default)]
pub struct Library(Mutex<Vec<(String, Value)>>);

impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_PROFILE_PODCASTS_PROOF").as_deref() == Ok("1")
    }

    pub fn report(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }

    pub fn request(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() {
            return None;
        }
        if path == "/api/v6/playlist/detail" {
            let (id, uid) = match payload["id"].as_str() {
                Some("20") => (20, 73),
                Some("21") => (21, 74),
                Some("22") => (22, 2),
                _ => return None,
            };
            return Some(Ok(json!({"code":200,"playlist":{
                "id":id,"userId":uid,"name":format!("合成播客歌单 {id}"),
                "creator":{"userId":uid,"nickname":author(uid)},"trackCount":0,
                "privacy":0,"trackIds":[],"tracks":[]}})));
        }
        let uid = path
            .strip_prefix("/api/w/v1/user/detail/")
            .and_then(|uid| uid.parse::<u64>().ok())
            .filter(|uid| [2, 73, 74].contains(uid));
        if uid.is_none()
            && ![
                COUNT, CREATED, COLLECTED, DETAIL, PROGRAMS, RESUME, LIKED, SSO,
            ]
            .contains(&path)
        {
            return None;
        }
        let attempt = {
            let mut calls = self.0.lock().unwrap();
            calls.push((path.into(), payload.clone()));
            calls
                .iter()
                .filter(|(p, body)| p == path && body == payload)
                .count()
        };
        assert!(!cache);
        if let Some(uid) = uid {
            assert_eq!(*payload, json!({"all":true,"userId":uid.to_string()}));
            return Some(Ok(json!({"code":200,"profile":{
                "userId":uid,"nickname":author(uid),"eventCount":3,
                "inBlacklist":uid == 74 && attempt > 1}})));
        }
        Some(Ok(match path {
            COUNT => {
                let uid = payload["userId"].as_str().unwrap();
                assert_eq!(*payload, json!({"userId":uid}));
                let count = if uid == "73" { 21 } else { 1 };
                json!({"code":200,"createRadioCount":count,"subRadioCount":count})
            }
            CREATED => {
                let uid = payload["userId"].as_str().unwrap();
                let offset = payload["offset"].as_u64().unwrap();
                assert_eq!(*payload, json!({"userId":uid,"offset":offset,"limit":20}));
                match (uid, offset, attempt) {
                    ("73", 0, _) => json!({"code":200,"count":21,"djRadios":
                        (801..=820).map(|id| radio(id, if id==820 {99} else {73})).collect::<Vec<_>>()}),
                    ("73", 20, 1) => json!({"code":503}),
                    ("73", 20, _) => json!({"code":200,"count":21,"djRadios":[radio(821,73)]}),
                    ("2", 0, _) => json!({"code":200,"count":1,"djRadios":[radio(701,2)]}),
                    _ => return Some(Err(Error::Response("unexpected created podcast read"))),
                }
            }
            COLLECTED => {
                let uid = payload["targetUserId"].as_str().unwrap();
                assert_eq!(*payload, json!({"targetUserId":uid,"limit":1000}));
                match (uid, attempt) {
                    ("73", 1) => json!({"code":503}),
                    ("73", _) => json!({"code":200,"count":21,"hasMore":false,"djRadios":
                        (901..=921).map(|id| radio(id, 85)).collect::<Vec<_>>()}),
                    ("2", _) => {
                        json!({"code":200,"count":1,"hasMore":false,"djRadios":[radio(702,85)]})
                    }
                    _ => return Some(Err(Error::Response("unexpected collected podcast read"))),
                }
            }
            DETAIL if *payload == json!({"id":"821"}) => {
                json!({"code":200,"data":radio(821,73)})
            }
            PROGRAMS if *payload == json!({"radioId":"821","offset":0,"limit":200,"asc":true}) => {
                json!({"code":200,"data":{"programs":[],"count":0,"more":false,"asc":true}})
            }
            RESUME if *payload == json!({"radioId":"821"}) => {
                json!({"code":200,"data":{"resumePlayValue":0}})
            }
            LIKED => json!({"code":200,"data":{"data":[],"hasMore":false}}),
            SSO if *payload == json!({}) && attempt == 1 => {
                json!({"token":"SYNTHETIC+PROFILE&SSO"})
            }
            SSO if *payload == json!({}) && attempt == 2 => json!({"code":503}),
            _ => return Some(Err(Error::Response("unexpected profile podcast request"))),
        }))
    }
}

fn author(uid: u64) -> String {
    format!("合成播客作者 {uid}")
}

fn radio(id: u64, uid: u64) -> Value {
    json!({"id":id,"name":format!("合成播客 {id}"),"programCount":0,
        "dj":{"userId":uid,"nickname":author(uid)},"subCount":17,"rawSecret":"PAYLOAD_PRIVATE"})
}
