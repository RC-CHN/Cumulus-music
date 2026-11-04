use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const FEED: &str = "/api/event/pc/history/feed/get";
const DELETE: &str = "/api/event/delete";
const ACK: &str = "90071992547409973331";
const LOST: &str = "90071992547409973332";
const ACK_DETAIL: &str = "/api/v1/event/2/90071992547409973331";
const LOST_DETAIL: &str = "/api/v1/event/2/90071992547409973332";

fn row(id: &str, owner: u32, label: &str, tombstone: bool) -> Value {
    json!({"id":id,"type":if tombstone {22} else {18},"eventTime":1700000000000_u64,
        "user":{"userId":owner,"nickname":if owner == 2 {"合成账号"} else {"来宾作者"}},
        "json":if tombstone {"{}".into()} else {json!({"title":label}).to_string()}})
}

#[derive(Default)]
pub struct Library(Mutex<Vec<(String, Value)>>);
impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_COMMUNITY_DELETE_PROOF").as_deref() == Ok("1")
    }
    pub fn report(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }
    pub fn request(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() || !matches!(path, FEED | DELETE | ACK_DETAIL | LOST_DETAIL) {
            return None;
        }
        assert!(!cache);
        let mut calls = self.0.lock().unwrap();
        calls.push((path.into(), payload.clone()));
        let attempt = calls
            .iter()
            .filter(|(called, body)| called == path && body == payload)
            .count();
        Some(match path {
            FEED if attempt == 1
                && (payload == &json!({"cursor":"-1"}) || payload == &json!({"cursor":-1})) =>
            {
                // Deliberately include stale acknowledged rows in the refresh.
                Ok(
                    json!({"code":200,"data":{"events":[row(ACK,2,"合成待删笔记",false), row(LOST,2,"合成未知笔记",false), row("77",73,"合成来宾笔记",false)],"more":false}}),
                )
            }
            ACK_DETAIL if attempt == 1 && payload == &json!({}) => {
                Ok(json!({"code":200,"event":row(ACK,2,"合成待删笔记",false)}))
            }
            DELETE if attempt == 1 && payload == &json!({"id":ACK}) => Ok(json!({"code":200})),
            DELETE if attempt == 1 && payload == &json!({"id":LOST}) => {
                Err(Error::Transport("synthetic delete reply lost".into()))
            }
            LOST_DETAIL if attempt == 1 && payload == &json!({}) => Ok(json!({"code":404})),
            LOST_DETAIL if attempt == 2 && payload == &json!({}) => {
                Ok(json!({"code":200,"event":row(LOST,2,"",true)}))
            }
            _ => Err(Error::InvalidInput(
                "unexpected synthetic note deletion request",
            )),
        })
    }
}
