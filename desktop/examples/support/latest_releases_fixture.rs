//! Closed catalogue fixtures used only by the WebView inspection example.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static FAIL_WEST: AtomicBool = AtomicBool::new(true);
static FAIL_MORE: AtomicBool = AtomicBool::new(true);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_LATEST_RELEASES_PROOF").is_some()
}

pub fn report() -> Value {
    json!(*CALLS.lock().unwrap())
}

pub fn response(path: &str, body: &Value) -> Option<Result<Value>> {
    if !enabled()
        || !matches!(
            path,
            "/api/v2/discovery/new/songs" | "/api/discovery/new/albums/area"
        )
    {
        return None;
    }
    CALLS.lock().unwrap().push(json!([path, body]));
    Some(if path.ends_with("new/songs") {
        match body["areaId"].as_str() {
            Some("96") if FAIL_WEST.swap(false, Ordering::AcqRel) => {
                Err(Error::Transport("synthetic region read failed".into()))
            }
            Some("0" | "96" | "7") if body["offset"] == 0 && body["limit"] == 100 => {
                let rows = (1..=100).map(|n| json!({
                    "id":n,"name":format!("新歌 {} · {}",body["areaId"].as_str().unwrap(),n),
                    "artists":[{"id":12,"name":"合成歌手"}],"album":{"id":7,"name":"测试专辑"},
                    "duration":235000,"privilege":{"pl":if n == if body["areaId"] == "96" {1} else {2} {0} else {128000},"flag":0}
                })).collect::<Vec<_>>();
                Ok(json!({"code":200,"data":rows}))
            }
            Some("8" | "16" | "536870911") => Ok(json!({"code":200,"data":[]})),
            _ => Err(Error::Response("invalid synthetic release area")),
        }
    } else {
        match (
            body["area"].as_str(),
            body["rcmd"].as_bool(),
            body["limit"].as_u64(),
            body["offset"].as_u64(),
        ) {
            (Some("ALL" | "ZH"), Some(true), Some(10), Some(0)) => Ok(json!({
                "code":200,"hasMore":true,
                "weekData":[{"id":7,"name":"测试专辑","artist":{"id":12,"name":"测试歌手"}}],
                "monthData":(10..20).map(album).collect::<Vec<_>>()
            })),
            (Some("ALL" | "ZH"), Some(true), Some(10), Some(10))
                if FAIL_MORE.swap(false, Ordering::AcqRel) =>
            {
                Err(Error::Transport(
                    "synthetic new-album continuation failed".into(),
                ))
            }
            (Some("ALL" | "ZH"), Some(true), Some(10), Some(10)) => Ok(json!({
                "code":200,"hasMore":false,"monthData":(20..30).map(album).collect::<Vec<_>>()
            })),
            (Some("ALL" | "ZH"), Some(false), Some(50), Some(0)) => Ok(json!({
                "code":200,"hasMore":true,"monthData":(100..150).map(album).collect::<Vec<_>>()
            })),
            (Some("ALL" | "ZH"), Some(false), Some(50), Some(50)) => Ok(json!({
                "code":200,"hasMore":false,"monthData":(150..170).map(album).collect::<Vec<_>>()
            })),
            _ => Err(Error::Response("invalid synthetic release pagination")),
        }
    })
}

fn album(id: u64) -> Value {
    json!({"id":id,"name":format!("发行专辑 {id}"),"size":5,
        "artists":[{"id":12,"name":"测试歌手"}]})
}
