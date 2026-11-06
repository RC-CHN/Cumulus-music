//! Closed signed-capability adapter for the real current-episode shelf proof.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

pub const REVISION: &str = "voice-recommendations-fixture-revision";
const READ: &str = "/api/voice/play/page/rcmd";

#[derive(Default)]
pub struct Fixture {
    requests: Mutex<Vec<Value>>,
    routes: Mutex<Vec<Value>>,
    old_pending: AtomicBool,
    release_old: Notify,
}

fn podcast(id: &str) -> Value {
    let id: u64 = id.parse().expect("numeric synthetic route");
    assert!(matches!(id, 98 | 121), "unexpected synthetic route target");
    json!({"id":id,"name":format!("合成推荐播单 {id}"),"programCount":1,"subed":false,
        "dj":{"userId":55,"nickname":"合成主播"}})
}

impl Fixture {
    pub async fn read(&self, path: &str, body: Value) -> Result<Value> {
        assert_eq!(path, READ);
        let voice = body["voiceId"]
            .as_str()
            .expect("canonical synthetic program");
        assert!(matches!(voice, "601" | "602"));
        assert_eq!(body, json!({"voiceId":voice}));
        let ordinal = {
            let mut requests = self.requests.lock().unwrap();
            requests.push(json!([path, body]));
            requests.len()
        };
        let (start, count) = match (ordinal, voice) {
            (1, "601") => (90, 10),
            (2, "601") => {
                self.old_pending.store(true, Ordering::Release);
                tokio::time::timeout(Duration::from_secs(30), self.release_old.notified())
                    .await
                    .map_err(|_| {
                        Error::Transport("synthetic old shelf release timed out".into())
                    })?;
                // Deliberately distinct data: must never be displayed.
                (140, 6)
            }
            (3, "602") => (120, 4),
            _ => panic!("unexpected repeat/out-of-order synthetic recommendation read"),
        };
        let data = (start..start + count)
            .map(|id| {
                json!({"id":id,"name":format!("合成推荐播单 {id}"),
                "playCount":10000 + id,"authorization":"SYNTHETIC_RECOMMENDATION_PRIVATE"})
            })
            .collect::<Vec<_>>();
        Ok(json!({"code":200,"data":data}))
    }

    pub fn old_pending(&self) -> bool {
        self.old_pending.load(Ordering::Acquire)
    }

    pub fn release(&self) {
        assert!(self.old_pending(), "no actual old recommendation request");
        self.release_old.notify_one();
    }

    pub fn snapshot(&self) -> Value {
        json!({"requests":self.requests.lock().unwrap().clone(),
            "routes":self.routes.lock().unwrap().clone(),"oldPending":self.old_pending()})
    }

    pub fn route(&self, path: &str, body: &Value, cache: bool) -> Option<Value> {
        let response = match path {
            "/api/djradio/v3/get" => {
                let id = body["id"].as_str().expect("canonical radio detail id");
                assert_eq!(*body, json!({"id":id}));
                json!({"code":200,"data":podcast(id)})
            }
            "/api/v6/dj/program/byradio" => {
                let id = body["radioId"].as_str().expect("canonical program list id");
                let radio = podcast(id);
                let id = radio["id"].as_u64().unwrap();
                assert_eq!(body["offset"], 0);
                assert!(body["asc"].is_boolean());
                json!({"code":200,"data":{"programs":[{"id":600 + id,"name":format!("合成推荐播单节目 {id}"),
                    "mainSong":{"id":1600 + id,"duration":30000},"duration":30000,"radio":radio}],
                    "count":1,"more":false,"asc":body["asc"]}})
            }
            "/api/content/my/liked/voice" => {
                assert_eq!(*body, json!({"limit":100,"likeTime":0}));
                json!({"code":200,"data":{"data":[],"hasMore":false}})
            }
            _ => return None,
        };
        assert!(!cache);
        self.routes.lock().unwrap().push(json!([path, body]));
        Some(response)
    }
}
