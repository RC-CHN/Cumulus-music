//! Isolated authorization failures and damaged preferences, never real HTTP.
use cumulus_core::{Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
static DENIED: AtomicBool = AtomicBool::new(false);
static REQUESTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_QUALITY_UI_PROOF").is_some()
}
pub fn response(args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    Some((|| {
        let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().expect("fixture ids"))?;
        REQUESTS
            .lock()
            .unwrap()
            .push(json!({"id":ids[0],"quality":args["level"]}));
        let denied = DENIED.load(Ordering::SeqCst) && args["level"] == "vivid";
        Ok(
            json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":if denied {403} else {200},"md5":"fixture","level":"standard","expi":120}]}),
        )
    })())
}
pub fn control(store: &StateStore, value: &Value) {
    match value["kind"].as_str().expect("fixture kind") {
        "deny" => DENIED.store(true, Ordering::SeqCst),
        "allow" => DENIED.store(false, Ordering::SeqCst),
        "damage" => store
            .put(
                "2",
                "playback-quality-v1",
                "current",
                &json!({"quality":"invalid"}),
            )
            .unwrap(),
        _ => panic!("unknown quality fixture operation"),
    }
}
pub fn report() -> Value {
    json!({"requests":*REQUESTS.lock().unwrap(),"denied":DENIED.load(Ordering::SeqCst)})
}
