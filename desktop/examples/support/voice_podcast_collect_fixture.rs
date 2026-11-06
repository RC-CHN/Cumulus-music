//! Sealed podcast ownership, collection state and the exact mutation trace.
use serde_json::{Value, json};
use std::sync::Mutex;

const MINE: u64 = 2;
const OTHER: u64 = 55;
const SHARED: u64 = 80;
const OWN: u64 = 81;
const SUBSCRIBE: &str = "/api/djradio/sub";
const UNSUBSCRIBE: &str = "/api/djradio/unsub";

#[derive(Default)]
pub struct Fixture {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    subscribed: bool,
    trace: Vec<Value>,
}

fn radio(id: u64, subscribed: bool) -> Value {
    let (name, owner, subscribers) = match id {
        SHARED => ("合成来访播客", OTHER, 42 + u64::from(subscribed)),
        OWN => ("合成自有播客", MINE, 7),
        _ => panic!("unexpected synthetic radio"),
    };
    json!({"id":id,"name":name,"subCount":subscribers,
        "programCount":1,"subed":subscribed,
        "dj":{"userId":owner,"nickname":format!("合成播客主播 {owner}")},
        "authorization":"SYNTHETIC_RADIO_PRIVATE"})
}

fn episode(value: &mut Value, subscribed: bool) {
    let (id, owner) = match value["id"].as_u64() {
        Some(601) => (SHARED, OTHER),
        Some(602) => (OWN, MINE),
        _ => panic!("unexpected synthetic episode"),
    };
    value["radio"] = radio(id, subscribed && id == SHARED);
    value["creator"] = json!({"userId":owner,"nickname":format!("合成播客主播 {owner}")});
}

impl Fixture {
    pub fn enrich(&self, mut response: Value) -> Value {
        let subscribed = self.state.lock().unwrap().subscribed;
        if let Some(rows) = response
            .get_mut("data")
            .and_then(Value::as_array_mut)
            .and_then(|rows| rows.first_mut())
            .and_then(|block| block.get_mut("creatives"))
            .and_then(Value::as_array_mut)
        {
            for row in rows {
                if let Some(programs) = row
                    .get_mut("creativeExtInfoVO")
                    .and_then(|value| value.get_mut("djPrograms"))
                    .and_then(Value::as_array_mut)
                {
                    for program in programs {
                        episode(program, subscribed);
                    }
                }
            }
        }
        if let Some(program) = response.get_mut("program") {
            episode(program, subscribed);
        }
        response
    }

    pub fn response(&self, path: &str, body: &Value, cache: bool) -> Option<Value> {
        if !matches!(
            path,
            "/api/djradio/get/subed"
                | "/api/djradio/v3/get"
                | "/api/v6/dj/program/byradio"
                | "/api/content/my/liked/voice"
                | SUBSCRIBE
                | UNSUBSCRIBE
        ) {
            return None;
        }
        assert!(!cache);
        let mut state = self.state.lock().unwrap();
        let before = state.subscribed;
        let response = match path {
            "/api/djradio/get/subed" => {
                assert_eq!(*body, json!({"targetUserId":"2","limit":1000}));
                let radios = if before {
                    vec![json!({"id":SHARED})]
                } else {
                    vec![]
                };
                json!({"code":200,"count":radios.len(),"hasMore":false,"djRadios":radios})
            }
            "/api/djradio/v3/get" => {
                let id = match body["id"].as_str() {
                    Some("80") => SHARED,
                    Some("81") => OWN,
                    _ => panic!("unexpected synthetic radio detail"),
                };
                assert_eq!(*body, json!({"id":id.to_string()}));
                json!({"code":200,"data":radio(id, before && id == SHARED)})
            }
            "/api/v6/dj/program/byradio" => {
                assert_eq!(body["radioId"], "80");
                assert_eq!(body["offset"], 0);
                assert!(body["asc"].is_boolean());
                let mut program = json!({"id":601,"name":"合成关联节目","mainSong":{"id":1601,"duration":30000},"duration":30000});
                episode(&mut program, before);
                json!({"code":200,"data":{"programs":[program],"count":1,"more":false,"asc":body["asc"]}})
            }
            "/api/content/my/liked/voice" => {
                assert_eq!(*body, json!({"limit":100,"likeTime":0}));
                json!({"code":200,"data":{"data":[],"hasMore":false}})
            }
            SUBSCRIBE | UNSUBSCRIBE => {
                assert_eq!(
                    *body,
                    json!({"id":"80"}),
                    "only the explicitly selected other-owned radio can be written"
                );
                assert_ne!(before, path == SUBSCRIBE, "no redundant or repeated writes");
                state.subscribed = path == SUBSCRIBE;
                json!({"code":200})
            }
            _ => unreachable!("closed podcast collection path"),
        };
        let after = state.subscribed;
        state
            .trace
            .push(json!({"path":path,"body":body,"before":before,"after":after}));
        Some(response)
    }

    pub fn trace(&self) -> Vec<Value> {
        self.state.lock().unwrap().trace.clone()
    }
}
