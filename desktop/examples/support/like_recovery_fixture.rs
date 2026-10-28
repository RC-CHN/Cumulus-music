//! Synthetic likes whose writes can be applied or lost before acknowledgement.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
pub struct Library {
    base: Arc<dyn Api>,
    liked: Mutex<BTreeSet<String>>,
    pub healthy: AtomicBool,
    pub fail_read: AtomicBool,
    pub writes: AtomicUsize,
    pub reads: AtomicUsize,
}
impl Library {
    pub fn new(base: Arc<dyn Api>) -> Self {
        Self {
            base,
            liked: Mutex::new(["3", "7", "10"].map(str::to_owned).into()),
            healthy: AtomicBool::new(false),
            fail_read: AtomicBool::new(false),
            writes: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
        }
    }
    pub fn remote(&self, id: &str, value: bool) {
        let mut set = self.liked.lock().unwrap();
        if value {
            set.insert(id.into());
        } else {
            set.remove(id);
        }
    }
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path == "/api/song/like/get" {
                self.reads.fetch_add(1, Ordering::SeqCst);
                if self.fail_read.swap(false, Ordering::SeqCst) {
                    return Err(Error::Transport("synthetic read failure".into()));
                }
                return Ok(json!({"code":200,"ids":*self.liked.lock().unwrap()}));
            }
            if path == "/api/song/like" {
                let id = args["trackId"].as_str().unwrap_or("");
                assert!(args["userid"] == "2" && ["2", "4", "5"].contains(&id));
                self.writes.fetch_add(1, Ordering::SeqCst);
                let healthy = self.healthy.load(Ordering::SeqCst);
                if id == "2" || healthy {
                    self.remote(id, args["like"].as_bool().unwrap());
                }
                return if healthy {
                    Ok(json!({"code":200}))
                } else {
                    Err(Error::Transport("synthetic lost like response".into()))
                };
            }
            self.base.request(path, args, cache).await
        })
    }
}
