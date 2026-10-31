//! Read-only synthetic service for the real production start coordinator.
use cumulus_core::Result;
use serde_json::{Value, json};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
#[derive(Default)]
struct Fixture {
    hold: AtomicBool,
    fail: AtomicBool,
    wake: Notify,
    requests: Mutex<Vec<Value>>,
    hold_catalog: AtomicBool,
    catalog_started: AtomicBool,
    removed_seed: AtomicBool,
}
fn fixture() -> &'static Fixture {
    static VALUE: OnceLock<Fixture> = OnceLock::new();
    VALUE.get_or_init(Fixture::default)
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_INTELLIGENCE_START_PROOF").is_some()
}
pub fn control(kind: &str) {
    assert!(enabled());
    let f = fixture();
    match kind {
        "observe" => {}
        "hold_catalog" => {
            f.catalog_started.store(false, Ordering::SeqCst);
            f.hold_catalog.store(true, Ordering::SeqCst);
        }
        "release_catalog" => {
            f.hold_catalog.store(false, Ordering::SeqCst);
            f.wake.notify_waiters();
        }
        "unlike_seed" => f.removed_seed.store(true, Ordering::SeqCst),
        "hold" => {
            f.hold.store(true, Ordering::SeqCst);
            f.fail.store(false, Ordering::SeqCst);
        }
        "success" => {
            f.hold.store(false, Ordering::SeqCst);
            f.fail.store(false, Ordering::SeqCst);
            f.wake.notify_waiters();
        }
        "fail" => {
            f.hold.store(false, Ordering::SeqCst);
            f.fail.store(true, Ordering::SeqCst);
            f.wake.notify_waiters();
        }
        _ => panic!("unknown intelligence fixture control"),
    }
}
pub fn report() -> Value {
    json!({"requests":fixture().requests.lock().unwrap().clone(),"catalogStarted":fixture().catalog_started.load(Ordering::SeqCst),"realAccountWrites":0,"realMusicRequests":0})
}
pub fn playlist(mut value: Value) -> Value {
    if enabled() && value["id"] == 1 && fixture().removed_seed.load(Ordering::SeqCst) {
        value["trackIds"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["id"] != 1);
        value["trackCount"] = json!(39);
    }
    value
}
pub async fn before(path: &str) {
    if !enabled() || path != "/api/v3/song/detail" {
        return;
    }
    let f = fixture();
    loop {
        let wake = f.wake.notified();
        tokio::pin!(wake);
        wake.as_mut().enable();
        if !f.hold_catalog.load(Ordering::SeqCst) {
            break;
        }
        f.catalog_started.store(true, Ordering::SeqCst);
        wake.await;
    }
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == "/api/music/dislike/get" {
        return Some(Ok(
            json!({"code":200,"data":{"songIdList":[],"artistIdList":[]}}),
        ));
    }
    if path != "/api/playmode/intelligence/list" {
        return None;
    }
    let f = fixture();
    f.requests.lock().unwrap().push(args.clone());
    loop {
        let wake = f.wake.notified();
        tokio::pin!(wake);
        wake.as_mut().enable();
        if !f.hold.load(Ordering::SeqCst) {
            break;
        }
        wake.await;
    }
    Some(Ok(if f.fail.load(Ordering::SeqCst) {
        json!({"code":500})
    } else {
        json!({"code":200,"data":[{"id":41,"songInfo":super::song(41),"alg":"synthetic","recommended":true},{"id":42,"songInfo":super::song(42),"recommended":true}]})
    }))
}
