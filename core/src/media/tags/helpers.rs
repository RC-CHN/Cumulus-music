use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy, trim_js},
};
use serde_json::{Map, Value};

pub(super) fn truth(value: Option<&Value>) -> bool {
    value.is_some_and(js_truthy)
}
pub(super) fn text(value: Option<&Value>) -> Result<&str> {
    if !truth(value) {
        return Ok("");
    }
    value
        .and_then(Value::as_str)
        .ok_or(Error::InvalidInput("metadata text must be a string"))
}
pub(super) fn trimmed(value: Option<&Value>) -> Result<String> {
    Ok(trim_js(text(value)?).into())
}
pub(super) fn default(value: Option<&Value>, fallback: Value) -> Value {
    value.filter(|v| js_truthy(v)).cloned().unwrap_or(fallback)
}
pub(super) fn copy(target: &mut Map<String, Value>, name: &str, source: Option<&Value>) {
    if let Some(value) = source {
        target.insert(name.into(), value.clone());
    }
}
pub(super) fn string(value: Option<&Value>) -> Result<String> {
    value
        .map(js_string)
        .unwrap_or_else(|| Ok("undefined".into()))
}
pub(super) fn artists(value: Option<&Value>) -> Result<Vec<Value>> {
    if !truth(value) {
        return Ok(Vec::new());
    }
    let entries = value
        .and_then(Value::as_array)
        .ok_or(Error::InvalidInput("artists must be an array"))?;
    let mut output = Vec::new();
    for artist in entries.iter().filter(|v| js_truthy(v)) {
        let name = trimmed(artist.get("name"))?;
        if !name.is_empty() {
            output.push(serde_json::json!([
                name,
                default(artist.get("id"), Value::from(0))
            ]));
        }
    }
    Ok(output)
}
pub(super) fn artist_text(value: Option<&Value>) -> Result<Value> {
    if !truth(value) || value.and_then(Value::as_array).is_some_and(Vec::is_empty) {
        return Ok(Value::Array(Vec::new()));
    }
    Ok(artists(value)?
        .iter()
        .map(|v| v[0].as_str().expect("artist text"))
        .collect::<Vec<_>>()
        .join("/")
        .into())
}
