//! Closed synthetic playlist transport for one group collector and lost ack.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";

#[derive(Default)]
struct State {
    calls: Vec<Value>,
    sent: bool,
    details_after_send: usize,
}

#[derive(Default)]
pub struct Fixture(Mutex<State>);

impl Fixture {
    pub fn read(&self, path: &str, body: &Value, cache: bool) -> Option<Value> {
        match path {
            LIST => {
                assert!(!cache);
                assert_eq!(
                    *body,
                    json!({"uid":"2","offset":0,"limit":10,"sortType":"default"})
                );
                self.0.lock().unwrap().calls.push(json!([path, body]));
                Some(json!({"code":200,"more":false,"playlist":[
                    {"id":81,"name":"合成已有收藏歌单","userId":2,"privacy":10,"trackCount":1},
                    {"id":82,"name":"合成喜欢列表","userId":2,"privacy":10,"specialType":5},
                    {"id":83,"name":"合成他人歌单","userId":3,"privacy":0}
                ]}))
            }
            DETAIL => {
                assert!(!cache);
                assert_eq!(*body, json!({"id":"81","n":100000,"s":0}));
                let mut state = self.0.lock().unwrap();
                if state.sent {
                    state.details_after_send += 1;
                    assert!(state.details_after_send <= 2, "no automatic reconciliation");
                }
                let observed = state.details_after_send == 2;
                state.calls.push(json!([path, body, observed]));
                Some(
                    json!({"code":200,"playlist":{"id":81,"name":"合成已有收藏歌单",
                        "userId":2,"privacy":10,"tags":[],"trackIds": if observed {
                            vec![json!({"id":1601}),json!({"id":1791}),json!({"id":1710}),json!({"id":701})]
                        } else { vec![json!({"id":701})] }
                    }}),
                )
            }
            ADD | "/api/playlist/create" => panic!("test forbids unguarded collection or create"),
            _ => None,
        }
    }

    pub fn add(&self, path: &str, body: Value) -> Result<Value> {
        assert_eq!(path, ADD);
        assert_eq!(
            body,
            json!({"pid":"81","op":"add","trackIds":"[\"1601\",\"1791\",\"1710\"]"})
        );
        let mut state = self.0.lock().unwrap();
        assert!(
            !state.sent,
            "no repeat group add after lost acknowledgement"
        );
        state.sent = true;
        state.calls.push(json!([path, body]));
        Err(Error::Transport(
            "synthetic lost group add acknowledgement".into(),
        ))
    }

    pub fn snapshot(&self) -> Value {
        json!({"calls":self.0.lock().unwrap().calls})
    }
}
