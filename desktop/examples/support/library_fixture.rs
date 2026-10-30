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
    removed: Vec<u64>,
    added: Vec<u64>,
    detail_reads: u64,
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
    json!({"reads":s.reads,"writes":s.writes,"detailReads":s.detail_reads,"removed":s.removed,"added":s.added})
}
fn playlist(id: u64) -> Value {
    let mut value = super::playlist(id);
    let own_count = if std::env::var_os("CUMULUS_SIDEBAR_UI_PROOF").is_some() {
        1200
    } else {
        60
    };
    let owner = if id <= own_count { 2 } else { 3 };
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
    if std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some() && id == 7 {
        value["trackIds"] = json!(
            (1..=240)
                .map(|id| json!({"id":id,"v":0}))
                .collect::<Vec<_>>()
        );
        value["trackCount"] = json!(240);
    }
    if std::env::var_os("CUMULUS_PLAYLIST_QUEUE_UI_PROOF").is_some() && id == 8 {
        value["trackIds"] = json!(
            (501..=504)
                .map(|id| json!({"id":id,"v":0}))
                .collect::<Vec<_>>()
        );
        value["trackCount"] = json!(4);
    }
    value
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    let mutation = std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some()
        && matches!(
            path,
            "/api/playlist/delete" | "/api/playlist/subscribe" | "/api/playlist/unsubscribe"
        );
    if !enabled()
        || !(mutation
            || matches!(
                path,
                "/api/user/playlist" | "/api/v6/playlist/detail" | "/api/playlist/order/update"
            ))
    {
        return None;
    }
    for _ in 0..400 {
        let mode = state().lock().unwrap().mode.clone();
        let held = mode == "hold" && path == "/api/user/playlist"
            || mode == "hold_detail" && path == "/api/v6/playlist/detail"
            || mode == "hold_write" && mutation;
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
            if s.mode == "fail" || s.mode == "fail_readback" {
                return Some(Err(Error::Transport("合成歌单读取失败".into())));
            }
            if s.mode == "expired" {
                return Some(Ok(json!({"code":301})));
            }
            let mut ids: Vec<u64> = match s.mode.as_str() {
                "empty" => vec![],
                "shrink" => vec![1, 2, 61],
                _ => (1..=if std::env::var_os("CUMULUS_SIDEBAR_UI_PROOF").is_some() {
                    2400
                } else {
                    90
                })
                    .collect(),
            };
            if !s.order.is_empty() && s.mode != "shrink" && s.mode != "empty" {
                let rest: Vec<_> = ids.into_iter().filter(|id| !s.order.contains(id)).collect();
                ids = s.order.clone();
                ids.extend(rest);
            }
            ids.retain(|id| !s.removed.contains(id));
            for id in &s.added {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(1000) as usize;
            Ok(
                json!({"code":200,"more":offset+limit<ids.len(),"playlist":ids.into_iter().skip(offset).take(limit).map(playlist).collect::<Vec<_>>()}),
            )
        }
        "/api/v6/playlist/detail" => {
            s.detail_reads += 1;
            if s.mode == "fail_detail" {
                return Some(Err(Error::Transport("合成歌单详情读取失败".into())));
            }
            if s.mode == "expired_detail" {
                return Some(Ok(json!({"code":301})));
            }
            if s.mode == "empty_detail" {
                return Some(Ok(json!({"code":200,"playlist":{"trackIds":[]}})));
            }
            Ok(json!({"code":200,"playlist":playlist(id(&args["id"]))}))
        }
        "/api/playlist/delete" | "/api/playlist/subscribe" | "/api/playlist/unsubscribe" => {
            assert!(mutation);
            s.writes += 1;
            if s.mode == "reject" {
                return Some(Ok(json!({"code":403})));
            }
            let id = id(&args[if path.ends_with("delete") {
                "pid"
            } else {
                "id"
            }]);
            assert!(id > 1, "never mutate the favorite list");
            if path.ends_with("/subscribe") {
                assert!(id > 60);
                s.removed.retain(|old| *old != id);
                s.added.push(id);
            } else {
                assert!(if path.ends_with("delete") {
                    id <= 60
                } else {
                    id > 60
                });
                s.removed.push(id);
                s.added.retain(|old| *old != id);
            }
            if s.mode == "uncertain" {
                s.mode = "fail_readback".into();
                return Some(Err(Error::Transport("合成写入后连接中断".into())));
            }
            Ok(json!({"code":200}))
        }
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
