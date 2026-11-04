use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const FEED: &str = "/api/event/pc/history/feed/get";
const REPORT: &str = "/api/report/reportevent";
const ACCEPTED: &str = "90071992547409973331";
const FORWARD: &str = "90071992547409973332";
const DUPLICATE: &str = "90071992547409973333";
const DETAIL_ACCEPTED: &str = "/api/v1/event/73/90071992547409973331";
const DETAIL_FORWARD: &str = "/api/v1/event/74/90071992547409973332";
const DETAIL_DUPLICATE: &str = "/api/v1/event/75/90071992547409973333";

fn event(id: &str, owner: u32, kind: u32, title: &str, attachment: Value) -> Value {
    let mut body = attachment;
    body["title"] = json!(title);
    json!({"id":id,"type":kind,"eventTime":1700000000000_u64,
        "user":{"userId":owner,"nickname":format!("合成作者 {owner}")},
        "srcResId":"90071992547409979999","srcResType":"note","uuid":"synthetic-ignored-uuid",
        "json":body.to_string()})
}
fn accepted() -> Value {
    event(
        ACCEPTED,
        73,
        19,
        "合成可举报笔记",
        json!({"album":{"id":97,"name":"合成附件专辑"}}),
    )
}
fn forwarded() -> Value {
    event(
        FORWARD,
        74,
        22,
        "合成可举报转发",
        json!({
            "event":event("90071992547409973399",88,18,"合成转发原文",json!({}))
        }),
    )
}
fn duplicate() -> Value {
    event(DUPLICATE, 75, 18, "合成已举报动态", json!({}))
}

#[derive(Default)]
pub struct Library(Mutex<Vec<(String, Value)>>);
impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_COMMUNITY_REPORT_PROOF").as_deref() == Ok("1")
    }
    pub fn report(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }
    pub fn request(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() {
            return None;
        }
        if matches!(
            path,
            "/api/event/delete"
                | "/api/share/friends/resource"
                | "/api/report/reportuser"
                | "/api/report/reportcomment"
        ) {
            panic!("unexpected synthetic moderation scope: {path}");
        }
        if !matches!(
            path,
            FEED | REPORT | DETAIL_ACCEPTED | DETAIL_FORWARD | DETAIL_DUPLICATE
        ) {
            return None;
        }
        assert!(!cache);
        self.0.lock().unwrap().push((path.into(), payload.clone()));
        Some(match path {
            FEED => {
                assert_eq!(payload, &json!({"cursor":"-1"}));
                let mut anonymous = event("81", 83, 18, "合成匿名笔记", json!({}));
                anonymous["anonymityInfo"] = json!({"encryptUserId":"synthetic-hidden"});
                Ok(json!({"code":200,"data":{"events":[
                    accepted(),forwarded(),duplicate(),event("82",2,18,"合成自己笔记",json!({})),
                    anonymous,event("84",83,21,"",json!({})),
                ],"more":false}}))
            }
            DETAIL_ACCEPTED | DETAIL_FORWARD | DETAIL_DUPLICATE => {
                assert_eq!(payload, &json!({}));
                Ok(json!({"code":200,"event":match path {
                    DETAIL_ACCEPTED => accepted(), DETAIL_FORWARD => forwarded(), _ => duplicate(),
                }}))
            }
            REPORT => {
                assert_eq!(payload.as_object().unwrap().len(), 3);
                match payload["id"].as_str() {
                    Some(ACCEPTED) => {
                        assert_eq!(payload["userid"], "73");
                        Ok(json!({"code":200}))
                    }
                    Some(FORWARD) => {
                        assert_eq!(payload["userid"], "74");
                        Err(Error::Transport("synthetic report reply lost".into()))
                    }
                    Some(DUPLICATE) => {
                        assert_eq!(payload["userid"], "75");
                        Ok(json!({"code":-1}))
                    }
                    _ => panic!("unexpected synthetic report target"),
                }
            }
            _ => unreachable!(),
        })
    }
}
