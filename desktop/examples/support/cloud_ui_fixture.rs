//! Synthetic cloud catalog; every request still crosses the production facade.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_CLOUD_UI_PROOF").is_some()
}
pub struct Fixture {
    rows: Mutex<Vec<Value>>,
    writes: Mutex<Vec<Vec<String>>>,
    fail: AtomicBool,
    held: AtomicBool,
    mode: AtomicUsize,
    release: Notify,
}
fn rows(count: usize) -> Vec<Value> {
    (1..=count).map(|i|json!({"songId":1000+i,"songName":format!("云盘合成歌曲 {i:03} · 用来核对长标题与操作布局"),"artist":format!("合成歌手 {}", i%7),"album":"一段很长的云盘专辑名称用于检查窄窗口","fileName":format!("cloud-{i:03}.flac"),"fileSize":12_345_678+i,"addTime":1_700_000_000_000u64+i as u64*1000,"bitrate":999000,"simpleSong":{"name":format!("云盘合成歌曲 {i:03} · 用来核对长标题与操作布局"),"ar":[{"name":format!("合成歌手 {}",i%7)}],"al":{"name":"一段很长的云盘专辑名称用于检查窄窗口"},"dt":235000}})).collect()
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            rows: Mutex::new(if enabled() { rows(405) } else { vec![] }),
            writes: Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            held: AtomicBool::new(false),
            mode: AtomicUsize::new(0),
            release: Notify::new(),
        }
    }
}
impl Fixture {
    pub fn configure(&self, v: &Value) {
        if let Some(count) = v["rows"].as_u64() {
            *self.rows.lock().unwrap() = rows(count as usize);
        }
        if let Some(x) = v["fail"].as_bool() {
            self.fail.store(x, Ordering::Release);
        }
        if let Some(x) = v["hold"].as_bool() {
            self.held.store(x, Ordering::Release);
            if !x {
                self.release.notify_one();
            }
        }
        if let Some(x) = v["mode"].as_u64() {
            self.mode.store(x as usize, Ordering::Release);
        }
    }
    pub fn report(&self) -> Value {
        json!({"writes":self.writes.lock().unwrap().clone(),"rows":self.rows.lock().unwrap().len(),"realRequests":0})
    }
    pub async fn response(&self, path: &str, payload: &Value) -> Option<Result<Value>> {
        if !enabled() {
            return None;
        }
        if path == "/api/v1/cloud/get" {
            if self.held.load(Ordering::Acquire) {
                self.release.notified().await;
            }
            if self.fail.load(Ordering::Acquire) {
                return Some(Ok(json!({"code":503})));
            }
            let rows = self.rows.lock().unwrap();
            let offset = payload["offset"].as_u64().unwrap() as usize;
            return Some(Ok(
                json!({"code":200,"size":123456789,"maxSize":1073741824,"hasMore":offset+200<rows.len(),"data":rows.iter().skip(offset).take(200).cloned().collect::<Vec<_>>()}),
            ));
        }
        if path == "/api/cloud/del" {
            let ids: Vec<String> =
                serde_json::from_str(payload["songIds"].as_str().unwrap()).unwrap();
            self.writes.lock().unwrap().push(ids.clone());
            let mode = self.mode.load(Ordering::Acquire);
            if mode == 1 {
                return Some(Ok(json!({"code":403})));
            }
            let success = if mode == 2 {
                ids[..1].to_vec()
            } else {
                ids.clone()
            };
            self.rows
                .lock()
                .unwrap()
                .retain(|r| !success.iter().any(|id| id == &r["songId"].to_string()));
            if mode == 3 {
                self.fail.store(true, Ordering::Release);
                return Some(Err(Error::Transport(
                    "synthetic cloud response lost".into(),
                )));
            }
            let failed = ids
                .into_iter()
                .filter(|id| !success.contains(id))
                .collect::<Vec<_>>();
            return Some(Ok(json!({"code":200,"succIds":success,"failIds":failed})));
        }
        None
    }
}
