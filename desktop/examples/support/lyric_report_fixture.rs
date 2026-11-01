use cumulus_core::{
    Error, Result,
    lyrics::reports::{SUBMIT_PATH, TYPES_PATH},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Default)]
pub struct Fixture {
    types_mode: AtomicUsize,
    write_mode: AtomicUsize,
    type_reads: AtomicUsize,
    writes: Mutex<Vec<Value>>,
    accepted: Mutex<Vec<Value>>,
    types_release: tokio::sync::Notify,
    write_release: tokio::sync::Notify,
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_LYRIC_REPORT_PROOF").is_some()
}
impl Fixture {
    pub fn configure(&self, v: &Value) {
        if let Some(mode) = v["typesMode"].as_u64() {
            self.types_mode.store(mode as usize, Ordering::Release);
        }
        if let Some(mode) = v["writeMode"].as_u64() {
            self.write_mode.store(mode as usize, Ordering::Release);
        }
        if v["releaseTypes"] == true {
            self.types_release.notify_one();
        }
        if v["releaseWrite"] == true {
            self.write_release.notify_one();
        }
    }
    pub fn report(&self) -> Value {
        json!({"typeReads":self.type_reads.load(Ordering::Acquire),"writes":self.writes.lock().unwrap().clone(),"accepted":self.accepted.lock().unwrap().clone(),"realRequests":0})
    }
    pub async fn response(&self, path: &str, payload: &Value) -> Option<Result<Value>> {
        if path == TYPES_PATH {
            let mode = self.types_mode.load(Ordering::Acquire);
            self.type_reads.fetch_add(1, Ordering::AcqRel);
            if mode == 3 {
                self.types_release.notified().await;
            }
            return Some(Ok(match mode {
                1 => json!({"code":503}),
                2 => json!({"code":200,"data":{}}),
                4 => {
                    json!({"code":200,"data":(4..44).map(|v|(v.to_string(),json!(format!("合成错误类型 {v}：用于检查长文本与窄窗口的完整布局")))).collect::<serde_json::Map<String,Value>>()})
                }
                _ => {
                    let frozen: Vec<cumulus_core::lyrics::reports::CorrectionType> =
                        serde_json::from_str(include_str!(
                            "../../../core/tests/fixtures/lyric_report_types.json"
                        ))
                        .unwrap();
                    let mut data = frozen
                        .into_iter()
                        .map(|v| (v.key, json!(v.label)))
                        .collect::<serde_json::Map<String, Value>>();
                    for key in ["1", "2", "3"] {
                        data.insert(key.into(), json!("synthetic excluded type"));
                    }
                    json!({"code":200,"data":data})
                }
            }));
        }
        if path == SUBMIT_PATH {
            let mode = self.write_mode.load(Ordering::Acquire);
            self.writes.lock().unwrap().push(payload.clone());
            if mode == 4 {
                return Some(Err(Error::RequestNotSent(Box::new(Error::State(
                    "synthetic report preparation failure",
                )))));
            }
            if mode == 1 {
                return Some(Ok(json!({"code":403})));
            }
            self.accepted.lock().unwrap().push(payload.clone());
            if mode == 3 {
                self.write_release.notified().await;
            }
            return Some(if mode == 2 {
                Err(Error::Transport("synthetic report response lost".into()))
            } else {
                Ok(json!({"code":200}))
            });
        }
        None
    }
}
pub fn lyrics(id: &Value) -> Value {
    match id.as_str().unwrap_or_default() {
        "8" => {
            json!({"code":200,"yrc":{"version":42,"lyric":"[1000,2000](1000,1000,0)合成(2000,1000,0)歌词"}})
        }
        "9" => json!({"code":200,"sgc":false}),
        "10" => json!({"code":200,"lrc":{"lyric":"[00:01]没有版本字段的合成歌词"}}),
        _ => {
            json!({"code":200,"lrc":{"version":17,"lyric":"[00:01]合成歌词一\n[00:02]合成歌词二"},"yrc":{"version":42,"lyric":"[1000,2000](1000,1000,0)合成(2000,1000,0)歌词"}})
        }
    }
}
