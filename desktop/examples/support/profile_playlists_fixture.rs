use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const LIST: &str = "/api/user/playlist";
const TOP: &str = "/api/user/playlist/getTop";
const COUNT: &str = "/api/djradio/get/create/sub/count";
const DETAIL: &str = "/api/v6/playlist/detail";

#[derive(Default)]
pub struct Library(Mutex<Vec<(String, Value)>>);

impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_PROFILE_PLAYLISTS_PROOF").as_deref() == Ok("1")
    }

    pub fn report(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }

    pub fn request(&self, path: &str, args: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() {
            return None;
        }
        let user = match path {
            "/api/w/v1/user/detail/2" => Some(2),
            "/api/w/v1/user/detail/73" => Some(73),
            "/api/w/v1/user/detail/74" => Some(74),
            _ => None,
        };
        if path == LIST && args["limit"] != 1000 {
            return None;
        }
        if user.is_none() && ![LIST, TOP, COUNT, DETAIL].contains(&path) {
            return None;
        }
        if path == DETAIL && !["20", "21", "22", "42"].contains(&args["id"].as_str().unwrap_or(""))
        {
            return None;
        }
        self.0.lock().unwrap().push((path.into(), args.clone()));
        if let Some(uid) = user {
            assert_eq!(*args, json!({"all":true,"userId":uid.to_string()}));
            assert!(!cache);
            return Some(Ok(json!({"code":200,"profile":{
                "userId":uid,"nickname":author(uid),"inBlacklist":uid==74}})));
        }
        Some(Ok(match path {
            COUNT if ["2", "73"].contains(&args["userId"].as_str().unwrap_or("")) => {
                assert!(!cache);
                json!({"code":200,"createRadioCount":0,"subRadioCount":0})
            }
            LIST if *args == json!({"uid":"2","offset":0,"limit":1000}) => {
                assert!(!cache);
                let mut star = playlist(41, 2, 10);
                star["specialType"] = json!(5);
                json!({"code":200,"more":false,"playlist":[star,playlist(22,2,0),playlist(42,2,10),
                    playlist(43,73,10),playlist(45,73,0)]})
            }
            LIST if *args == json!({"uid":"73","offset":0,"limit":1000}) => {
                assert!(!cache);
                json!({"code":200,"more":false,"playlist":[playlist(20,73,0),playlist(25,73,0),
                    playlist(26,73,0),playlist(27,73,10),playlist(28,2,10),playlist(29,2,0)]})
            }
            TOP if *args == json!({"userId":"2","offset":0,"limit":1000}) => {
                assert!(!cache);
                json!({"code":200,"data":{"topItemList":[{"id":42,"name":"RAW_PRIVATE_PIN"},
                    playlist(46,2,0),playlist(47,2,10)]}})
            }
            TOP if *args == json!({"userId":"73","offset":0,"limit":1000}) => {
                assert!(!cache);
                json!({"code":200,"data":{"topItemList":[{"id":26},
                    playlist(27,73,0),playlist(28,2,10)]}})
            }
            DETAIL => {
                let (id, uid, privacy) = match args["id"].as_str() {
                    Some("20") => (20, 73, 0),
                    Some("21") => (21, 74, 0),
                    Some("22") => (22, 2, 0),
                    Some("42") => (42, 2, 10),
                    _ => unreachable!(),
                };
                json!({"code":200,"playlist":playlist(id,uid,privacy)})
            }
            _ => {
                return Some(Err(Error::Response(
                    "unexpected profile playlist proof request",
                )));
            }
        }))
    }
}

fn author(uid: u32) -> String {
    format!("合成歌单作者 {uid}")
}

fn playlist(id: u32, uid: u32, privacy: u8) -> Value {
    json!({"id":id,"userId":uid,"name":format!("合成主页歌单 {id}"),
        "creator":{"userId":uid,"nickname":author(uid)},"trackCount":0,
        "playCount":1234,"privacy":privacy,"trackIds":[],"tracks":[]})
}
