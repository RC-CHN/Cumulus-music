//! Closed read-only response pages for the native liked-programme waterfall.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

const LIST: &str = "/api/content/my/liked/voice";
const COUNT: &str = "/api/content/my/liked/voice/count";
const FIRST_TAIL: &str = "1700000000900";
const SECOND_TAIL: u64 = 1_700_000_000_863;
const TOTAL: u64 = 145;
const COMMENTS_ID: u64 = 1122;

#[derive(Default)]
pub struct Fixture {
    calls: Mutex<Vec<Value>>,
    first: AtomicUsize,
    second: AtomicUsize,
    third: AtomicUsize,
    reject: Notify,
}

impl Fixture {
    pub fn trace(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }

    pub fn reject_held_page(&self) {
        self.reject.notify_one();
    }

    pub async fn read(&self, path: &str, body: &Value, cache: bool) -> Option<Result<Value>> {
        self.calls.lock().unwrap().push(json!([path, body, cache]));
        match path {
            LIST => {
                assert!(!cache);
                Some(self.list(body).await)
            }
            COUNT => {
                assert!(!cache);
                assert_eq!(body, &json!({}));
                Some(Ok(json!({"code":200,"data":TOTAL})))
            }
            "/api/dj/program/detail" => {
                assert_eq!(body, &json!({"id":COMMENTS_ID.to_string()}));
                Some(Ok(json!({"code":200,"program":voice(COMMENTS_ID)})))
            }
            "/api/v1/resource/comments/A_DJ_1_1122"
            | "/api/v1/resource/hotcomments/A_DJ_1_1122" => Some(Ok(json!({
                "code":200,"comments":[],"hotComments":[],"topComments":[],
                "more":false,"hasMore":false,"total":0
            }))),
            // This example never gives a write path to the synthetic account.
            _ if path.contains("submit")
                || path.contains("/add")
                || path.contains("/delete")
                || path.contains("/update") && path != super::RECENT
                || path.contains("/clear")
                || path.starts_with("/api/content/my/liked/voice/")
                || matches!(path, "/api/song/like" | "/api/voice/like") =>
            {
                panic!("waterfall unexpectedly sent a synthetic write: {path}")
            }
            _ => None,
        }
    }

    async fn list(&self, body: &Value) -> Result<Value> {
        if body == &json!({"limit":100,"likeTime":0}) {
            assert_eq!(self.first.fetch_add(1, Ordering::SeqCst), 0);
            let mut rows = (1001..=1100).map(voice).collect::<Vec<_>>();
            rows.last_mut().unwrap()["likeTime"] = json!(FIRST_TAIL);
            return Ok(page(rows, true));
        }
        if body == &json!({"limit":100,"likeTime":FIRST_TAIL}) {
            return match self.second.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    self.reject.notified().await;
                    Err(Error::Transport("synthetic liked page interrupted".into()))
                }
                1 => {
                    // Repeat the tail row as the server can do; it must not
                    // take a second spot in the displayed prefix.
                    Ok(page((1100..=1137).map(voice).collect(), true))
                }
                _ => panic!("unexpected first continuation replay"),
            };
        }
        assert_eq!(body, &json!({"limit":100,"likeTime":SECOND_TAIL}));
        assert_eq!(self.third.fetch_add(1, Ordering::SeqCst), 0);
        Ok(page((1138..=1145).map(voice).collect(), false))
    }
}

fn voice(id: u64) -> Value {
    let mut voice = super::voice(id);
    voice["likeTime"] = json!(1_700_000_002_000_u64 - id);
    voice
}

fn page(rows: Vec<Value>, more: bool) -> Value {
    json!({"code":200,"data":{"data":rows,"hasMore":more}})
}
