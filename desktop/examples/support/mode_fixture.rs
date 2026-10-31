//! Isolated mode UI faults and a synthetic FM backend; no network fallback.
#[allow(dead_code)]
#[path = "../../../core/tests/support/fm.rs"]
mod fm_domain;
use cumulus_core::{Result, protocol::api::Api, storage::StateStore};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MODE_UI_PROOF").is_some()
}
fn backend() -> &'static fm_domain::Backend {
    static BACKEND: OnceLock<fm_domain::Backend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        let backend = fm_domain::Backend::default();
        if std::env::var_os("CUMULUS_EMPTY_FM_UI_PROOF").is_none() {
            backend.batch(&(101..=140).collect::<Vec<_>>());
        }
        backend
    })
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == "/api/music-vip-membership/client/vip/info" {
        return Some(Ok(
            json!({"code":200,"data":{"userId":2,"now":1793410000000u64}}),
        ));
    }
    if !matches!(
        path,
        "/api/v1/radio/get" | "/api/v3/radio/trash/get" | "/api/music/dislike/get"
    ) {
        return None;
    }
    Some(backend().request(path, args.clone(), false).await)
}
pub fn report() -> Value {
    json!({"fmReads":backend().count("/api/v1/radio/get"),"skipWrites":backend().count("/api/v1/radio/skip")})
}
pub fn control(store: &StateStore, kind: &str) {
    match kind {
        "refill_warning" => {
            store
                .put(
                    "2",
                    "fm-player-v1",
                    "failure",
                    &json!({"code":"refill_failed"}),
                )
                .unwrap();
            return;
        }
        "clear_warning" => {
            store.delete("2", "fm-player-v1", "failure").unwrap();
            return;
        }
        "empty_desired" | "restore_desired" => {
            desired_queue(store, kind == "empty_desired");
            return;
        }
        _ => {}
    }
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch(match kind {
        "reject"=>"CREATE TRIGGER reject_mode BEFORE UPDATE ON state WHEN NEW.owner='2' AND NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic mode commit failure'); END;",
        "allow"=>"DROP TRIGGER reject_mode;",
        _=>panic!("unknown isolated mode fixture"),
    }).unwrap();
}

// Model the recovery interval after the domain queue committed but before a
// native queue update succeeded. Only the standalone synthetic profile exists.
fn desired_queue(store: &StateStore, empty: bool) {
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    let mut query = sql
        .prepare("SELECT key,value FROM state WHERE owner='2' AND kind='fm-queues-v1'")
        .unwrap();
    let (key, mut queue): (String, Value) = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (key, value) = row.unwrap();
            (key, serde_json::from_str::<Value>(&value).unwrap())
        })
        .find(|(_, queue)| queue["source"] == json!({"kind":"personal"}))
        .expect("synthetic FM queue missing");
    let next = queue["revision"].as_u64().unwrap().checked_add(1).unwrap();
    if empty {
        assert!(
            store
                .get("2", "mode-ui-proof", "desired")
                .unwrap()
                .is_none()
        );
        store.put("2", "mode-ui-proof", "desired", &queue).unwrap();
        queue["tracks"] = json!([]);
        queue["index"] = json!(0);
    } else {
        queue = store.get("2", "mode-ui-proof", "desired").unwrap().unwrap();
        store.delete("2", "mode-ui-proof", "desired").unwrap();
    }
    queue["revision"] = json!(next);
    store.put("2", "fm-queues-v1", &key, &queue).unwrap();
}
