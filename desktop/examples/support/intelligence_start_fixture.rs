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
    json!({"requests":fixture().requests.lock().unwrap().clone(),"realAccountWrites":0,"realMusicRequests":0})
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
