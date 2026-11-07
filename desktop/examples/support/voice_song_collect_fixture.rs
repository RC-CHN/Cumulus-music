//! Closed synthetic playlist transport for one group collector and lost ack.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const HOST_LIST: &str = "/api/user/playlist";
const CREATE: &str = "/api/playlist/create";
const CREATED_NAME: &str = "合成新收藏歌单";

#[derive(Default)]
struct State {
    calls: Vec<Value>,
    sent: bool,
    details_after_send: usize,
    created: bool,
    lists_after_create: usize,
}

pub struct Fixture(Mutex<State>, Option<Creation>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Creation {
    Recover,
    AddFalse,
}

impl Fixture {
    pub fn new(creation: Option<&str>) -> Self {
        let mode = creation.map(|value| match value {
            "recover" => Creation::Recover,
            "add_false" => Creation::AddFalse,
            _ => panic!("invalid sealed group-create case"),
        });
        Self(Mutex::default(), mode)
    }

    pub fn create_enabled(&self) -> bool {
        self.1.is_some()
    }

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
            HOST_LIST if self.create_enabled() && body["includeVideo"] == true => {
                assert!(!cache);
                assert_eq!(
                    *body,
                    json!({"uid":"2","includeVideo":true,"limit":200,"offset":0})
                );
                let mut state = self.0.lock().unwrap();
                let mut rows =
                    vec![json!({"id":81,"name":"合成已有收藏歌单","userId":2,"privacy":10})];
                if state.created {
                    state.lists_after_create += 1;
                }
                let observed = self.1 == Some(Creation::AddFalse) && state.lists_after_create >= 1
                    || self.1 == Some(Creation::Recover) && state.lists_after_create >= 2;
                if observed {
                    rows.push(json!({"id":91,"name":CREATED_NAME,"userId":2,"privacy":10}));
                }
                state.calls.push(json!([path, body, observed]));
                Some(json!({"code":200,"more":false,"playlist":rows}))
            }
            DETAIL if self.create_enabled() => {
                assert!(!cache);
                assert_eq!(*body, json!({"id":"91","n":100000,"s":0}));
                let mut state = self.0.lock().unwrap();
                assert!(state.created);
                let observed = state.sent;
                state.calls.push(json!([path, body, observed]));
                let tracks = if observed {
                    vec![json!({"id":1601}), json!({"id":1791}), json!({"id":1710})]
                } else {
                    vec![]
                };
                Some(json!({"code":200,"playlist":{"id":91,"name":CREATED_NAME,
                    "userId":2,"creator":{"userId":2},"privacy":10,"tags":[],"trackIds":tracks}}))
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
            ADD | CREATE => panic!("test forbids unguarded collection or create"),
            _ => None,
        }
    }

    pub fn add(&self, path: &str, body: Value) -> Result<Value> {
        assert_eq!(path, ADD);
        if let Some(mode) = self.1 {
            assert_eq!(
                body,
                json!({"pid":"91","op":"add","trackIds":"[\"1601\",\"1791\",\"1710\"]"})
            );
            let mut state = self.0.lock().unwrap();
            assert!(state.created && !state.sent);
            assert_eq!(
                state.lists_after_create,
                if mode == Creation::Recover { 2 } else { 1 }
            );
            state.sent = true;
            state.calls.push(json!([path, body]));
            return Ok(json!({"code":if mode == Creation::Recover { 200 } else { 403 }}));
        }
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

    pub fn create(&self, path: &str, body: Value) -> Value {
        assert!(self.create_enabled());
        assert_eq!(path, CREATE);
        assert_eq!(body, json!({"uid":"2","name":CREATED_NAME,"privacy":10}));
        let mut state = self.0.lock().unwrap();
        assert!(!state.created, "no duplicate group create");
        state.created = true;
        state.calls.push(json!([path, body]));
        json!({"code":200,"id":91})
    }

    pub fn snapshot(&self) -> Value {
        json!({"calls":self.0.lock().unwrap().calls})
    }
}
