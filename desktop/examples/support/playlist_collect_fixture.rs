//! Isolated collection UI server. No live account or music endpoints are used.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

#[derive(Default)]
struct State {
    mode: String,
    created: BTreeMap<u64, Value>,
    tracks: BTreeMap<u64, Vec<Value>>,
    writes: u64,
    creates: u64,
    hidden: Option<u64>,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_COLLECT_PROOF").is_some()
}
pub fn control(value: &Value) {
    let mut s = state().lock().unwrap();
    s.mode = value["mode"].as_str().unwrap_or("success").into();
    if s.mode == "success" {
        s.hidden = None;
    }
}
pub fn report() -> Value {
    let s = state().lock().unwrap();
    json!({"writes":s.writes,"creates":s.creates,"created":s.created.keys().collect::<Vec<_>>()})
}
fn list(s: &State, id: u64) -> Value {
    let mut value = s
        .created
        .get(&id)
        .cloned()
        .unwrap_or_else(|| super::playlist(id));
    if let Some(ids) = s.tracks.get(&id) {
        value["trackIds"] = json!(
            ids.iter()
                .map(|id| json!({"id":id,"v":0}))
                .collect::<Vec<_>>()
        );
        value["trackCount"] = json!(ids.len());
    }
    if id == 2 && !s.tracks.contains_key(&id) {
        value["trackIds"] = json!([]);
        value["trackCount"] = json!(0);
    }
    if s.mode == "many" {
        value["userId"] = json!(2);
        value["creator"]["userId"] = json!(2);
    }
    value
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled()
        || !matches!(
            path,
            "/api/user/playlist"
                | "/api/v6/playlist/detail"
                | "/api/playlist/create"
                | "/api/v1/playlist/manipulate/tracks"
        )
    {
        return None;
    }
    for _ in 0..400 {
        let held = {
            let s = state().lock().unwrap();
            s.mode == "hold_list" && path == "/api/user/playlist"
                || s.mode == "hold_add" && path == "/api/v1/playlist/manipulate/tracks"
        };
        if !held {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let mut s = state().lock().unwrap();
    let id = |v: &Value| {
        v.as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| v.as_u64())
            .unwrap()
    };
    Some(match path {
        "/api/user/playlist" => {
            if s.mode == "fail_list" {
                return Some(Err(Error::Transport("合成歌单列表暂时不可用".into())));
            }
            let mut rows: Vec<_> = if s.mode == "empty" {
                vec![]
            } else {
                (1..=if s.mode == "many" { 260 } else { 20 })
                    .map(|id| list(&s, id))
                    .collect()
            };
            rows.extend(s.created.keys().map(|id| list(&s, *id)));
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(1000) as usize;
            Ok(
                json!({"code":200,"more":offset+limit<rows.len(),"playlist":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>()}),
            )
        }
        "/api/v6/playlist/detail" => {
            let id = id(&args["id"]);
            if s.hidden == Some(id) {
                return Some(Err(Error::Transport("合成收藏结果暂不可读".into())));
            }
            Ok(json!({"code":200,"playlist":list(&s,id)}))
        }
        "/api/playlist/create" => {
            assert_eq!(
                args["privacy"], 10,
                "probe creates only private synthetic lists"
            );
            s.creates += 1;
            let id = 1000 + s.creates;
            let mut value = super::playlist(id);
            value["userId"] = json!(2);
            value["creator"]["userId"] = json!(2);
            value["name"] = args["name"].clone();
            value["trackIds"] = json!([]);
            value["trackCount"] = json!(0);
            s.created.insert(id, value.clone());
            s.tracks.insert(id, vec![]);
            Ok(json!({"code":200,"playlist":value,"id":id}))
        }
        _ => {
            let id = id(&args["pid"]);
            assert!(id == 2 || s.created.contains_key(&id));
            assert_eq!(args["op"], "add");
            s.writes += 1;
            if s.mode == "reject_add" {
                return Some(Ok(json!({"code":403})));
            }
            let ids: Vec<Value> = serde_json::from_str(args["trackIds"].as_str().unwrap()).unwrap();
            let tracks = s.tracks.entry(id).or_default();
            for id in ids {
                if !tracks.contains(&id) {
                    tracks.push(id);
                }
            }
            if s.mode == "uncertain" {
                s.hidden = Some(id);
                return Some(Err(Error::Transport("合成收藏确认丢失".into())));
            }
            Ok(json!({"code":200}))
        }
    })
}
