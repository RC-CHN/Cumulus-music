//! Sealed native input and request receipt for the opt-in chorus WebView proof.
use cumulus_core::{
    Error, Result,
    protocol::api::{PlaybackChorusPermission, PlaybackChorusRead},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const REVISION: &str = "synthetic-chorus-queue-cookie";
static BATCHES: Mutex<Vec<Vec<Value>>> = Mutex::new(Vec::new());
static SINGLES: AtomicUsize = AtomicUsize::new(0);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PLAYER_CHORUS_QUEUE_PROOF").is_some()
}
pub fn revision() -> Option<String> {
    enabled().then(|| REVISION.to_owned())
}
pub fn batch(
    read: PlaybackChorusRead,
    revision: &str,
    permission: &PlaybackChorusPermission<'_>,
) -> Result<Value> {
    if !enabled() || revision != REVISION {
        return Err(Error::CredentialChanged);
    }
    permission()?;
    let (path, payload) = read.into_request()?;
    assert_eq!(path, "/api/song/chorus");
    let ids: Vec<Value> =
        serde_json::from_str(payload["ids"].as_str().expect("encoded fixture IDs"))?;
    assert_eq!(ids, [json!("1"), json!("2")]);
    BATCHES.lock().unwrap().push(ids);
    Ok(json!({"code":200,"data":[{"id":1,"startTime":4000},{"id":2,"startTime":13000}]}))
}
pub fn unexpected_single() -> Result<Value> {
    SINGLES.fetch_add(1, Ordering::SeqCst);
    Err(Error::State(
        "chorus queue proof unexpectedly used the single request",
    ))
}
pub fn report() -> Value {
    json!({"batches":*BATCHES.lock().unwrap(),"singles":SINGLES.load(Ordering::SeqCst)})
}
