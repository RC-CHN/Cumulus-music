//! Synthetic read-only history for the daily page shortcut proof.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static DAILY: AtomicUsize = AtomicUsize::new(0);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_DAILY_HISTORY_PROOF").is_some()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if matches!(
        path,
        "/api/v1/discovery/recommend/songs" | "/api/v3/discovery/recommend/songs"
    ) {
        CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
        if path != "/api/v1/discovery/recommend/songs" || *args != json!({"limit":30}) {
            return Some(Err(Error::InvalidInput(
                "wrong daily endpoint or arguments",
            )));
        }
        if DAILY.fetch_add(1, Ordering::Relaxed) == 1 {
            return Some(Err(Error::Transport("synthetic daily read outage".into())));
        }
        return Some(Ok(
            json!({"code":200,"recommend":(1..=40).map(super::song).collect::<Vec<_>>() }),
        ));
    }
    let data = match path {
        "/api/rtrs/abt/front/expinfo/list" if *args == json!({"expNames":"PH-PC-Blacklist"}) => {
            json!([{"expName":"PH-PC-Blacklist","expGroupName":"c"}])
        }
        "/api/music-vip-membership/client/vip/info" => json!({"userId":2,"now":1793410000000u64}),
        "/api/discovery/recommend/songs/history/recent" if *args == json!({}) => {
            json!({"dates":["2026-10-26"]})
        }
        "/api/discovery/recommend/songs/history/detail"
            if *args == json!({"date":"2026-10-26"}) =>
        {
            json!({"songs":[{"id":901,"name":"合成历史日推单曲","ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"dt":180000}]})
        }
        path if path.starts_with("/api/discovery/recommend/songs/history/") => {
            return Some(Err(Error::Response("unexpected daily history request")));
        }
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(Ok(json!({"code":200,"data":data})))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
