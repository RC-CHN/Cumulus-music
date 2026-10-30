//! Isolated metadata server for the editor's real IPC/save/reconcile UI proof.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
#[derive(Default)]
struct State {
    edits: BTreeMap<u64, Value>,
    mode: String,
    writes: u64,
    last_id: u64,
    fail_read: u64,
}
fn state() -> &'static Mutex<State> {
    static VALUE: OnceLock<Mutex<State>> = OnceLock::new();
    VALUE.get_or_init(|| Mutex::new(State::default()))
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_EDITOR_PROOF").is_some()
}
pub fn decorate(mut value: Value, id: u64) -> Value {
    if enabled()
        && let Some(edit) = state().lock().unwrap().edits.get(&id)
    {
        for key in ["name", "description", "tags"] {
            if let Some(v) = edit.get(key) {
                value[key] = v.clone();
            }
        }
    }
    value
}
pub fn control(value: &Value) {
    let mut s = state().lock().unwrap();
    s.mode = value["mode"].as_str().unwrap_or("success").to_owned();
    s.fail_read = value["failRead"].as_u64().unwrap_or(0);
}
pub fn fails_read(id: u64) -> bool {
    if !enabled() {
        return false;
    }
    let mut s = state().lock().unwrap();
    if s.fail_read == id {
        s.fail_read = 0;
        return true;
    }
    s.mode == "uncertain" && s.writes > 0 && s.last_id == id
}
pub fn writes() -> u64 {
    state().lock().unwrap().writes
}
pub async fn catalogue() -> Result<Value> {
    wait_while("hold_tags").await;
    let mode = state().lock().unwrap().mode.clone();
    if mode == "fail_tags" {
        return Err(Error::Response("synthetic tag catalogue unavailable"));
    }
    if mode == "empty_tags" {
        return Ok(json!({"code":200,"categories":{},"sub":[]}));
    }
    let mut rows = vec![
        json!({"name":"轻音乐","category":0}),
        json!({"name":"摇滚","category":0}),
        json!({"name":"古典","category":0}),
        json!({"name":"爵士","category":0}),
    ];
    let mut categories = json!({"0":"风格"});
    if mode == "rich_tags" {
        categories = json!({"0":"风格","1":"语种","2":"场景","3":"情感","4":"主题"});
        for (index, category) in ["风格", "语种", "场景", "情感", "主题"].iter().enumerate()
        {
            for number in 1..=if index == 0 { 40 } else { 12 } {
                rows.push(json!({"name":format!("{category}标签 {number:02}"),"category":index}));
            }
        }
        rows.push(json!({"name":"一段用来检查完整名称提示的很长标签","category":4}));
    }
    Ok(json!({"code":200,"categories":categories,"sub":rows}))
}
pub fn batch(args: Value) -> Result<Value> {
    let mut s = state().lock().unwrap();
    let name: Value = serde_json::from_str(args["/api/playlist/update/name"].as_str().unwrap())?;
    let tags: Value = serde_json::from_str(args["/api/playlist/tags/update"].as_str().unwrap())?;
    let desc: Value = serde_json::from_str(args["/api/playlist/desc/update"].as_str().unwrap())?;
    let id = name["id"]
        .as_str()
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| name["id"].as_u64())
        .unwrap();
    assert!(
        (2..20).contains(&id),
        "only the synthetic owned lists may be edited"
    );
    s.writes += 1;
    s.last_id = id;
    let reject = s.mode == "reject";
    let partial = s.mode == "partial";
    if !reject {
        let edit = s.edits.entry(id).or_insert(json!({}));
        edit["name"] = name["name"].clone();
        if !partial {
            edit["description"] = desc["desc"].clone();
            edit["tags"] = json!(
                tags["tags"]
                    .as_str()
                    .unwrap()
                    .split(';')
                    .filter(|v| !v.is_empty())
                    .collect::<Vec<_>>()
            );
        }
    }
    if s.mode == "uncertain" {
        return Err(Error::Response("synthetic lost metadata acknowledgment"));
    }
    Ok(
        json!({"code":200,"/api/playlist/update/name":{"code":if reject {403} else {200}},"/api/playlist/tags/update":{"code":if reject || partial {403} else {200}},"/api/playlist/desc/update":{"code":if reject || partial {403} else {200}}}),
    )
}

/// The probe releases the held read/write after observing its loading frame.
/// The cap prevents a broken WebView from leaving a fixture request pending.
pub async fn wait_while(mode: &str) {
    if !enabled() {
        return;
    }
    for _ in 0..800 {
        if state().lock().unwrap().mode != mode {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
