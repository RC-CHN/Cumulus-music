use super::super::response::KINDS;
use crate::{Error, Result, protocol::wire::serialization::js_truthy, storage::StateStore};
use serde_json::{Value, json};

pub(super) const KIND: &str = "lyrics_v1";

pub(super) fn read(store: &StateStore, owner: &str, key: &str) -> Result<Option<Value>> {
    store
        .get(owner, KIND, key)?
        .map(|v| {
            if v["schema"] != 1 || !v["response"].is_object() {
                return Err(Error::Response("invalid normalized lyric cache"));
            }
            Ok(v["response"].clone())
        })
        .transpose()
}

pub(super) fn record(value: &Value) -> Value {
    json!({"schema":1,"response":value})
}

/// M/3081 cache loader migration, applied only to explicit official-cache import.
/// Our versioned normalized cache must not be fed through this again.
pub(super) fn legacy(mut value: Value) -> Result<Option<Value>> {
    if !value.is_object() {
        return Err(Error::InvalidInput(
            "official lyric cache must be an object",
        ));
    }
    if js_truthy(&value["yrc"]) && !js_truthy(&value["yromalrc"]) && !js_truthy(&value["ytlrc"]) {
        return Ok(None);
    }
    for key in KINDS {
        if let Some(offset) = value[key].get("offset").filter(|v| js_truthy(v)) {
            let offset = offset
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or(Error::Response("invalid cached lyric offset"))?;
            if offset > 500.0 {
                value[key]["offset"] = json!(offset / 1000.0);
            }
        }
    }
    Ok(Some(value))
}
