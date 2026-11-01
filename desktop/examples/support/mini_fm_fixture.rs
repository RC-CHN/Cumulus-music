//! Synthetic FM service for the two-window probe. No network fallback.
#[allow(dead_code)]
#[path = "../../../core/tests/support/fm.rs"]
mod domain;
use cumulus_core::{Result, protocol::api::Api, storage::StateStore};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MINI_FM_PROOF").is_some()
}
pub struct Fixture {
    api: domain::Backend,
    reason_style: AtomicU8,
    corruption: Mutex<Option<(Arc<StateStore>, String)>>,
    corrupted: AtomicBool,
}
impl Default for Fixture {
    fn default() -> Self {
        let api = domain::Backend::default();
        if enabled() {
            api.batch(&(101..=140).collect::<Vec<_>>());
        }
        Self {
            api,
            reason_style: AtomicU8::new(0),
            corruption: Mutex::new(None),
            corrupted: AtomicBool::new(false),
        }
    }
}
impl Fixture {
    pub async fn response(&self, path: &str, args: &Value) -> Option<Result<Value>> {
        if !enabled() {
            return None;
        }
        if path == "/api/music-vip-membership/client/vip/info" {
            return Some(Ok(
                json!({"code":200,"data":{"userId":2,"now":1793410000000u64}}),
            ));
        }
        if matches!(
            path,
            "/api/v1/radio/get"
                | "/api/tag/tab/fm/recommends"
                | "/api/v1/radio/skip"
                | "/api/v3/radio/trash/get"
                | "/api/music/dislike/get"
                | "/api/personalized/dislike/reason"
                | "/api/radio/trash/add"
                | "/api/radio/trash/del/batch"
                | "/api/music-blacklist/add"
                | "/api/music-blacklist/delete"
        ) {
            let mut result = self.api.request(path, args.clone(), false).await;
            if path == "/api/personalized/dislike/reason"
                && let Ok(value) = result.as_mut()
            {
                if self.reason_style.load(Ordering::Acquire) == 1 {
                    value["data"] = json!([]);
                } else {
                    value["data"][2]["reason"] =
                        json!(format!("屏蔽歌手：{}", "长名称的合成歌手".repeat(14)));
                }
            }
            if path == "/api/radio/trash/add" {
                let target = self.corruption.lock().unwrap().take();
                if let Some((store, owner)) = target {
                    store
                        .put(
                            &owner,
                            "fm-blacklist-v1",
                            "current",
                            &json!({"songs":[],"artists":"synthetic unreadable"}),
                        )
                        .unwrap();
                    self.corrupted.store(true, Ordering::Release);
                }
            }
            if path == "/api/music/dislike/get" && result.is_ok() {
                self.corrupted.store(false, Ordering::Release);
            }
            return Some(result);
        }
        None
    }
    pub fn report(&self) -> Value {
        json!({"personalReads":self.api.count("/api/v1/radio/get"),"zoneReads":self.api.count("/api/tag/tab/fm/recommends"),"skipWrites":self.api.count("/api/v1/radio/skip"),"trashWrites":self.api.count("/api/radio/trash/add"),"reasonReads":self.api.count("/api/personalized/dislike/reason"),"artistWrites":self.api.count("/api/music-blacklist/add"),"remoteTrash":self.api.trash.lock().unwrap().clone(),"remoteArtists":self.api.blacklist.lock().unwrap().artists.clone(),"corrupted":self.corrupted.load(Ordering::Acquire)})
    }
    pub fn control(&self, store: &Arc<StateStore>, owner: &str, kind: &str) {
        match kind {
            "report" => {}
            "hold-reasons" => {
                *self.api.hold.lock().unwrap() = Some("/api/personalized/dislike/reason".into())
            }
            "release-reasons" | "release-write" => self.api.release.notify_one(),
            "hold-write" => *self.api.hold.lock().unwrap() = Some("/api/radio/trash/add".into()),
            "reason-error" => self
                .api
                .fault("/api/personalized/dislike/reason", domain::Fault::Before),
            "reason-empty" => self.reason_style.store(1, Ordering::Release),
            "reason-normal" => self.reason_style.store(0, Ordering::Release),
            "trash-rejected" => self
                .api
                .fault("/api/radio/trash/add", domain::Fault::Reject(403)),
            "trash-lost" => self.api.fault("/api/radio/trash/add", domain::Fault::Lost),
            "artist-rejected" => self
                .api
                .fault("/api/music-blacklist/add", domain::Fault::Reject(403)),
            "refresh-error" => self
                .api
                .fault("/api/music/dislike/get", domain::Fault::Before),
            "corrupt-next-write" => {
                *self.corruption.lock().unwrap() = Some((store.clone(), owner.into()))
            }
            _ => panic!("unknown synthetic mini feedback control"),
        }
    }
}
