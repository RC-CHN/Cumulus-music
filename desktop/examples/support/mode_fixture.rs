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
        backend.batch(&(101..=140).collect::<Vec<_>>());
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
        _ => {}
    }
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch(match kind {
        "reject"=>"CREATE TRIGGER reject_mode BEFORE UPDATE ON state WHEN NEW.owner='2' AND NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic mode commit failure'); END;",
        "allow"=>"DROP TRIGGER reject_mode;",
        _=>panic!("unknown isolated mode fixture"),
    }).unwrap();
}
