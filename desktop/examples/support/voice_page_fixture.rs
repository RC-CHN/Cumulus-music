//! Sealed typed VoicePage reads. Playback-detail requests keep their own trace.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Duration};
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Detail,
    Recommendations,
    Songs,
}

impl Lane {
    fn wire(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Detail => ("detail", "/api/dj/program/detail", "id"),
            Self::Recommendations => ("recommendations", "/api/voice/play/page/rcmd", "voiceId"),
            Self::Songs => ("songs", "/api/v1/dj/program/tracks", "id"),
        }
    }
}

pub struct Fixture {
    joined_proof: bool,
    requests: Mutex<Vec<Request>>,
    release: Notify,
}

struct Request {
    lane: Lane,
    program: u64,
    state: &'static str,
}

pub struct Ticket {
    pub program: u64,
    index: usize,
    ordinal: usize,
    held: bool,
}

impl Fixture {
    pub fn new(joined_proof: bool) -> Self {
        Self {
            joined_proof,
            requests: Mutex::default(),
            release: Notify::new(),
        }
    }

    pub fn begin(&self, lane: Lane, path: &str, payload: &Value) -> Ticket {
        let (_, expected, key) = lane.wire();
        assert_eq!(path, expected);
        let program = match payload[key].as_str() {
            Some("601") => 601,
            Some("602") => 602,
            _ => panic!("unexpected synthetic current program"),
        };
        assert_eq!(*payload, json!({key:program.to_string()}));
        let mut requests = self.requests.lock().unwrap();
        let ordinal = requests
            .iter()
            .filter(|request| request.lane == lane)
            .count();
        let held = self.joined_proof && lane == Lane::Detail && ordinal == 0;
        let index = requests.len();
        assert!(index < 128, "unexpected synthetic VoicePage reads");
        requests.push(Request {
            lane,
            program,
            state: "pending",
        });
        Ticket {
            program,
            index,
            ordinal,
            held,
        }
    }

    pub fn finish(&self, ticket: Ticket, result: Result<Value>) -> Result<Value> {
        let mut requests = self.requests.lock().unwrap();
        let request = &mut requests[ticket.index];
        assert_eq!(request.state, "pending");
        request.state = if result.is_ok() { "success" } else { "failure" };
        result
    }

    pub async fn detail(&self, ticket: &Ticket, mut response: Value) -> Result<Value> {
        if ticket.held {
            tokio::time::timeout(Duration::from_secs(30), self.release.notified())
                .await
                .expect("explicit failure fixture was not released");
            return Err(Error::Response("sealed fresh voice detail refusal"));
        }
        if self.joined_proof {
            response["program"]["name"] = json!("合成新读节目标题");
            response["program"]["description"] = json!("本段只来自重新读取的节目详情。");
            response["program"]["listenerCount"] = json!("0");
            response["program"]["coverUrl"] =
                json!("https://p1.music.126.net/never-replace-selected-art.png");
            if ticket.ordinal == 2 {
                response["program"]["creator"] = json!({"userId":56,"nickname":"合成仅展示主播"});
                response["program"]["radio"] =
                    json!({"id":81,"name":"合成仅展示播单","subCount":9});
            }
        }
        Ok(response)
    }

    pub fn recommendations(&self) -> Value {
        let rows = if self.joined_proof {
            (91..95)
                .map(|id| json!({"id":id,"name":format!("合成三路推荐 {id}"),"playCount":id}))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        json!({"code":200,"data":rows})
    }

    pub fn songs(&self) -> Value {
        let rows = if self.joined_proof {
            vec![json!({"id":1710,"name":"合成三路包含歌曲","dt":30000,
                "al":{"id":55,"name":"合成专辑"},
                "privilege":{"id":1710,"st":0,"pl":128000}})]
        } else {
            Vec::new()
        };
        json!({"code":200,"tracks":rows})
    }

    pub fn release_failure(&self) {
        assert!(self.joined_proof);
        let requests = self.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|request| request.program == 601));
        assert!(requests.iter().all(|request| {
            request.state
                == if request.lane == Lane::Detail {
                    "pending"
                } else {
                    "success"
                }
        }));
        self.release.notify_one();
    }

    pub fn snapshot(&self) -> Value {
        let requests = self.requests.lock().unwrap();
        Value::Array(requests.iter().map(|request| json!({
            "lane":request.lane.wire().0,"voiceId":request.program.to_string(),"state":request.state
        })).collect())
    }
}
