//! M/3070 0.1.5-beta.4: parameter composition, not PCM processing or permission.
mod defaults;
mod merge;
mod numeric;
mod rules;
pub use defaults::{Enhancement, builtin_base, builtin_diy, compose_builtin};

use crate::{
    Error, Result,
    protocol::wire::serialization::{js_json, js_truthy, trim_js},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: &str = "0.1.5-beta.4";
pub const BUILD_TIME: &str = "2026-01-22T10:43:31.283Z";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Composition {
    /// Official multiple-base-key success has no mergedJson at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_json: Option<Value>,
    pub used_diy_config: Vec<String>,
    pub is_use_zone_effect: bool,
}

fn parsed(value: Option<&Value>, empty: Value) -> Result<Value> {
    match value {
        Some(Value::String(value)) if trim_js(value).is_empty() => Ok(empty),
        Some(Value::String(value)) => Ok(serde_json::from_str(value)?),
        Some(value) => Ok(value.clone()),
        None => Ok(empty),
    }
}

fn valid_document(value: &Value) -> bool {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text).is_ok_and(|v| v.is_object()),
        Value::Object(_) => true,
        _ => false,
    }
}

fn ordered_keys(values: &Map<String, Value>) -> Result<Vec<String>> {
    let canonical: Value = serde_json::from_str(&js_json(&Value::Object(values.clone()))?)?;
    Ok(canonical
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect())
}

/// Accept the official object/JSON-string fields, including its truthy
/// supportStereo behavior when source+diy validation bypasses the boolean check.
/// Arrays/primitive documents and non-finite computed material numbers are
/// rejected at the Rust boundary rather than emulating JS property boxing.
pub fn compose(input: &Value) -> Result<Composition> {
    // Bound copies, string parsing and the number of enhancement entries.
    if serde_json::to_vec(input)?.len() > 4 * 1024 * 1024 {
        return Err(Error::InvalidInput("composition input exceeds limit"));
    }
    let input = input
        .as_object()
        .ok_or(Error::InvalidInput("composition requires an object"))?;
    let base = input.get("baseJson");
    let song = input.get("songJson");
    let diy = input.get("diyJson");
    let stereo = input.get("supportStereo");
    if !base.is_some_and(js_truthy) && !song.is_some_and(js_truthy) {
        return Err(Error::InvalidInput(
            "composition needs baseJson or songJson",
        ));
    }
    let checked_source = song
        .filter(|v| js_truthy(v))
        .or_else(|| base.filter(|v| js_truthy(v)));
    if let Some(source) = checked_source.filter(|_| diy.is_some_and(js_truthy)) {
        if !valid_document(source) || !diy.is_some_and(valid_document) {
            return Err(Error::InvalidInput(
                "invalid composition source or DIY document",
            ));
        }
    } else if stereo.is_some_and(|v| !v.is_null() && !v.is_boolean()) {
        return Err(Error::InvalidInput(
            "supportStereo must be boolean without source and DIY",
        ));
    }
    // Official code parses both source fields, including a losing base string.
    let base = parsed(base, Value::Null)?;
    let song = parsed(song, Value::Null)?;
    let diy = match diy {
        Some(Value::String(text)) if trim_js(text).is_empty() => Value::Object(Map::new()),
        _ => parsed(diy, Value::Null)?,
    };
    for source in [&base, &song] {
        if js_truthy(source) && !source.is_object() {
            return Err(Error::InvalidInput("composition source must be an object"));
        }
    }
    let diy = diy
        .as_object()
        .ok_or(Error::InvalidInput("composition DIY must be an object"))?;
    if diy.len() > 256 {
        return Err(Error::InvalidInput(
            "too many composition enhancement entries",
        ));
    }
    let stereo = stereo.is_some_and(js_truthy);
    let filtered = rules::filter(&base, &song, diy, stereo)?;
    let merged = merge::merge(&base, &song, &filtered, stereo)?;
    let zone = merged.as_ref().is_some_and(|value| {
        value.pointer("/aev2/rvb/on") == Some(&Value::Bool(true))
            || value.pointer("/aev2/se/on") == Some(&Value::Bool(true))
    });
    Ok(Composition {
        merged_json: merged,
        used_diy_config: ordered_keys(&filtered)?,
        is_use_zone_effect: zone,
    })
}
