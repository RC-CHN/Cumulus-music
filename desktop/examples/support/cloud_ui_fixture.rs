//! Synthetic cloud catalog; every request still crosses the production facade.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_CLOUD_UI_PROOF").is_some() || batch_enabled()
}
pub fn menu_enabled() -> bool {
    std::env::var_os("CUMULUS_CLOUD_MENU_UI_PROOF").is_some()
}
pub fn batch_enabled() -> bool {
    std::env::var_os("CUMULUS_CLOUD_BATCH_UI_PROOF").is_some()
}
#[path = "cloud_batch_media.rs"]
pub mod media;
pub struct Fixture {
    rows: Mutex<Vec<Value>>,
    capacity: Mutex<(Value, Value)>,
    writes: Mutex<Vec<Vec<String>>>,
    fail: AtomicBool,
    held: AtomicBool,
    mode: AtomicUsize,
    release: Notify,
    collection: Mutex<Vec<String>>,
    collections: Mutex<Vec<Value>>,
    collect_fail: AtomicBool,
    download_auths: Mutex<Vec<String>>,
}
fn rows(count: usize) -> Vec<Value> {
    (1..=count).map(|i| {
        let artist = format!("合成歌手 {}", i % 7);
        let artists = if i == 5 && !batch_enabled() {
            json!([{"name":""},{"name":"合成合作歌手"}])
        } else {
            json!([{"name":artist}])
        };
        json!({"songId":1000+i,"songName":format!("云盘合成歌曲 {i:03} · 用来核对长标题与操作布局"),"artist":artist,"album":"一段很长的云盘专辑名称用于检查窄窗口","fileName":format!("cloud-{i:03}.flac"),"fileSize":if batch_enabled(){media::DATA.len()}else{12_345_678+i},"addTime":1_700_000_000_000u64+i as u64*1000,"bitrate":999000,"simpleSong":{"id":if menu_enabled(){json!(900000+i)}else{Value::Null},"t":if menu_enabled()&&i==2{1}else{0},"privilege":{"st":if menu_enabled()&&i==3{-100}else{0}},"name":format!("云盘合成歌曲 {i:03} · 用来核对长标题与操作布局"),"ar":artists,"al":{"name":"一段很长的云盘专辑名称用于检查窄窗口"},"dt":235000}})
    }).collect()
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            rows: Mutex::new(if enabled() { rows(405) } else { vec![] }),
            capacity: Mutex::new((json!(123456789), json!(1073741824))),
            writes: Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            held: AtomicBool::new(false),
            mode: AtomicUsize::new(0),
            release: Notify::new(),
            collection: Mutex::new((1..=40).map(|id| id.to_string()).collect()),
            collections: Mutex::new(vec![]),
            collect_fail: AtomicBool::new(false),
            download_auths: Mutex::new(vec![]),
        }
    }
}
impl Fixture {
    pub fn configure(&self, v: &Value) {
        if let Some(kinds) = v["deletionKinds"].as_object() {
            let mut rows = self.rows.lock().unwrap();
            for (id, kind) in kinds {
                let id: u64 = id.parse().unwrap();
                let row = rows
                    .iter_mut()
                    .find(|row| row["songId"].as_u64() == Some(id))
                    .unwrap();
                row["simpleSong"]["privilege"]["cs"] = json!(true);
                row["simpleSong"]["t"] = json!(match kind.as_str().unwrap() {
                    "ordinary" => 0,
                    "private" => 1,
                    "off_store" => 3,
                    _ => panic!("unknown synthetic cloud deletion type"),
                });
            }
        }
        for (key, index) in [("size", 0), ("maxSize", 1)] {
            if let Some(value) = v.get(key) {
                assert!(value.is_null() || value.as_u64().is_some());
                let mut capacity = self.capacity.lock().unwrap();
                if index == 0 {
                    capacity.0 = value.clone();
                } else {
                    capacity.1 = value.clone();
                }
            }
        }
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
        if let Some(x) = v["collectFail"].as_bool() {
            self.collect_fail.store(x, Ordering::Release);
        }
    }
    pub fn report(&self) -> Value {
        json!({"writes":self.writes.lock().unwrap().clone(),"rows":self.rows.lock().unwrap().len(),"realRequests":0,
            "collection":self.collection.lock().unwrap().clone(),"collectWrites":self.collections.lock().unwrap().clone(),
            "downloadAuths":self.download_auths.lock().unwrap().clone(),"mediaRequests":media::requests()})
    }
    pub async fn response(&self, path: &str, payload: &Value) -> Option<Result<Value>> {
        if !enabled() {
            return None;
        }
        if batch_enabled() {
            if path == "/api/user/playlist" {
                let count = self.collection.lock().unwrap().len();
                let playlists = (1..=24)
                    .map(|id| {
                        let mut playlist = super::playlist(id);
                        if id == 2 {
                            playlist["trackCount"] = json!(count);
                        }
                        playlist
                    })
                    .collect::<Vec<_>>();
                return Some(Ok(json!({"code":200,"more":false,"playlist":playlists})));
            }
            if path == "/api/v6/playlist/detail" && payload["id"] == "2" {
                let ids = self.collection.lock().unwrap();
                let mut playlist = super::playlist(2);
                playlist["trackCount"] = json!(ids.len());
                playlist["trackIds"] = json!(
                    ids.iter()
                        .map(|id| json!({"id":id,"v":0}))
                        .collect::<Vec<_>>()
                );
                return Some(Ok(json!({"code":200,"playlist":playlist})));
            }
            if path == "/api/v1/playlist/manipulate/tracks" {
                assert_eq!(payload["pid"], "2");
                self.collections.lock().unwrap().push(payload.clone());
                if self.collect_fail.load(Ordering::Acquire) {
                    return Some(Ok(json!({"code":403})));
                }
                assert_eq!(payload["op"], "add");
                let ids: Vec<String> =
                    serde_json::from_str(payload["trackIds"].as_str().unwrap()).unwrap();
                let mut collection = self.collection.lock().unwrap();
                for id in ids {
                    if !collection.contains(&id) {
                        collection.push(id);
                    }
                }
                return Some(Ok(json!({"code":200})));
            }
            if path == "/api/cloud/dowonload" {
                let id = payload["songId"].as_str().unwrap();
                let row = self
                    .rows
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|r| r["songId"].as_u64() == id.parse::<u64>().ok())
                    .cloned()
                    .expect("owned cloud original");
                self.download_auths.lock().unwrap().push(id.into());
                return Some(Ok(
                    json!({"code":200,"songId":id,"name":row["fileName"],"size":media::DATA.len(),"url":media::URL,"md5":"6b1c7d8698e1e941ad6f2046faa0b611"}),
                ));
            }
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
            return Some(Ok({
                let (size, max_size) = self.capacity.lock().unwrap().clone();
                json!({"code":200,"size":size,"maxSize":max_size,"hasMore":offset+200<rows.len(),"data":rows.iter().skip(offset).take(200).cloned().collect::<Vec<_>>()})
            }));
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
