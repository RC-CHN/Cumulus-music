//! Synthetic grouped library for the real WebView, playback and order UI.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};
#[derive(Default)]
struct State {
    mode: String,
    order: Vec<u64>,
    reads: u64,
    writes: u64,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_LIBRARY_UI_PROOF").is_some()
}
pub fn control(value: &Value) {
    state().lock().unwrap().mode = value["mode"].as_str().unwrap_or("success").into();
}
pub fn report() -> Value {
    let s = state().lock().unwrap();
    json!({"reads":s.reads,"writes":s.writes})
}
fn playlist(id: u64) -> Value {
    let mut value = super::playlist(id);
    let owner = if id <= 60 { 2 } else { 3 };
    value["userId"] = json!(owner);
    value["creator"]["userId"] = json!(owner);
    value["creator"]["nickname"] = json!(if owner == 2 {
        "合成创建者"
    } else {
        "合成收藏来源 · 非真实用户"
    });
    value["privacy"] = json!(if id == 6 { 10 } else { 0 });
    if id == 5 {
        value["trackIds"] = json!([]);
        value["trackCount"] = json!(0);
    }
    value
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled()
        || !matches!(
            path,
            "/api/user/playlist" | "/api/v6/playlist/detail" | "/api/playlist/order/update"
        )
    {
        return None;
    }
    for _ in 0..400 {
        let held = state().lock().unwrap().mode == "hold" && path == "/api/user/playlist";
        if !held {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let mut s = state().lock().unwrap();
    let id = |v: &Value| {
        v.as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| v.as_u64())
            .unwrap()
    };
    Some(match path {
        "/api/user/playlist" => {
            s.reads += 1;
            if s.mode == "fail" {
                return Some(Err(Error::Transport("合成歌单读取失败".into())));
            }
            if s.mode == "expired" {
                return Some(Ok(json!({"code":301})));
            }
            let mut ids: Vec<u64> = match s.mode.as_str() {
                "empty" => vec![],
                "shrink" => vec![1, 2, 61],
                _ => (1..=90).collect(),
            };
            if !s.order.is_empty() && s.mode != "shrink" && s.mode != "empty" {
                let rest: Vec<_> = ids.into_iter().filter(|id| !s.order.contains(id)).collect();
                ids = s.order.clone();
                ids.extend(rest);
            }
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(1000) as usize;
            Ok(
                json!({"code":200,"more":offset+limit<ids.len(),"playlist":ids.into_iter().skip(offset).take(limit).map(playlist).collect::<Vec<_>>()}),
            )
        }
        "/api/v6/playlist/detail" => Ok(json!({"code":200,"playlist":playlist(id(&args["id"]))})),
        _ => {
            let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
            let ids: Vec<u64> = ids.iter().map(id).collect();
            assert_eq!(ids.len(), 60);
            assert_eq!(ids[0], 1);
            assert!(ids.iter().all(|id| (1..=60).contains(id)));
            s.order = ids;
            s.writes += 1;
            Ok(json!({"code":200}))
        }
    })
}
