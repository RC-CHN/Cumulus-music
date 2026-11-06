//! Closed, opt-in badge transport. Keeps stale positive server counts after
//! both replies so the production native observation/consumption path matters.
use cumulus_core::{
    Error, Result,
    protocol::api::{PodcastBadgeClear, PodcastBadgePermission},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

pub const REVISION: &str = "sealed-synthetic-podcast-badge";
const CLEAR: &str = "/api/djradio/newcount/clear/byradio";

#[derive(Default)]
pub struct Fixture {
    trace: Mutex<Vec<Value>>,
    recommendations: bool,
}
impl Fixture {
    pub fn new(recommendations: bool) -> Self {
        Self {
            recommendations,
            ..Default::default()
        }
    }

    pub fn trace(&self) -> Vec<Value> {
        self.trace.lock().unwrap().clone()
    }

    pub fn read(&self, path: &str, body: &Value, cache: bool) -> Option<Result<Value>> {
        const LIST: &str = "/api/my/podcast/page/list";
        const DETAIL: &str = "/api/djradio/v3/get";
        let value = match path {
            "/api/djradio/my/radio/recommend" => {
                assert!(!cache);
                assert_eq!(body, &json!({}));
                if self.recommendations {
                    self.trace.lock().unwrap().push(json!([path, body]));
                    let cards = (81..=87)
                        .map(|id| {
                            json!({
                                "id":id, "name":format!("合成推荐 {id}"), "playCount":1870
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({"code":200,"data":cards})
                } else {
                    json!({"code":200,"data":[]})
                }
            }
            LIST => {
                assert!(!cache);
                assert_eq!(body, &json!({"selectType":1,"sortType":1}));
                self.trace.lock().unwrap().push(json!([LIST, body]));
                json!({"code":200,"data":{"radioIdList":[81,82,83],
                    "myPodcastVOList":[radio(81),radio(82),radio(83)]}})
            }
            DETAIL => {
                let id = id(&body["id"]);
                assert_eq!(body, &json!({"id":id.to_string()}));
                self.trace.lock().unwrap().push(json!([DETAIL, body]));
                json!({"code":200,"data":radio(id)})
            }
            "/api/v6/dj/program/byradio" => {
                let id = id(&body["radioId"]);
                assert_eq!(body["offset"], 0);
                if body["limit"] == 250 {
                    assert!(self.recommendations && id == 81);
                    assert_eq!(body, &json!({"radioId":"81","offset":0,"limit":250}));
                    self.trace.lock().unwrap().push(json!([path, body]));
                }
                json!({"code":200,"data":{"programs":[program(id)],"count":1,"more":false,"asc":body["asc"]}})
            }
            "/api/dj/program/detail" if self.recommendations => {
                assert_eq!(body, &json!({"id":"9081"}));
                self.trace.lock().unwrap().push(json!([path, body]));
                json!({"code":200,"program":program(81)})
            }
            "/api/dj/play/url/check" if self.recommendations => {
                assert_eq!(body["voiceId"], "9081");
                self.trace.lock().unwrap().push(json!([path, body]));
                json!({"code":200,"data":{"voiceId":9081,"songId":10081,"canPlay":true}})
            }
            "/api/song/enhance/player/url/v1" if self.recommendations => {
                let ids: Value = serde_json::from_str(body["ids"].as_str().unwrap()).unwrap();
                assert!(ids == json!([10081]) || ids == json!(["10081"]));
                self.trace.lock().unwrap().push(json!([path, body]));
                json!({"code":200,"data":[{"id":10081,"url":super::MEDIA,"code":200,
                    "md5":"fixture","level":"standard","expi":120}]})
            }
            CLEAR | "/api/djradio/newcount/clear" => {
                panic!("badge write escaped the sealed typed fixture")
            }
            _ => return None,
        };
        Some(Ok(value))
    }

    pub fn clear(
        &self,
        write: PodcastBadgeClear,
        revision: &str,
        permission: &PodcastBadgePermission<'_>,
        dispatched: &AtomicBool,
    ) -> Result<()> {
        assert_eq!(revision, REVISION);
        let (path, body) = write.into_request();
        assert_eq!(path, CLEAR);
        assert!(body == json!({"radioId":"81"}) || body == json!({"radioId":"82"}));
        permission()?;
        dispatched.store(true, Ordering::Relaxed);
        let mut trace = self.trace.lock().unwrap();
        let previous = trace.iter().filter(|row| row[0] == CLEAR).count();
        assert_eq!(body["radioId"], if previous == 0 { "81" } else { "82" });
        assert!(
            previous < 2,
            "an already-attempted badge clear was sent again"
        );
        let confirmed = body["radioId"] == "81";
        trace.push(json!([path, body]));
        if confirmed {
            Ok(())
        } else {
            Err(Error::Transport("synthetic badge reply lost".into()))
        }
    }
}

fn id(value: &Value) -> u64 {
    let id: u64 = value.as_str().unwrap().parse().unwrap();
    assert!((81..=83).contains(&id));
    id
}
fn radio(id: u64) -> Value {
    let (name, count) = match id {
        81 => ("合成确认播客", 108),
        82 => ("合成待核对播客", 4),
        83 => ("合成无更新播客", 0),
        _ => unreachable!(),
    };
    json!({"id":id,"name":name,"newVoiceCount":count,
        "lastProgramCreateTime":1_700_000_000_123u64,"programCount":1,"subed":true})
}

fn program(id: u64) -> Value {
    json!({"id":9000+id,"name":format!("合成单集 {id}"),
        "mainSong":{"id":10000+id,"duration":30000},"duration":30000,"radio":radio(id)})
}
