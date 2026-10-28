//! Official M/21 N/R catalog metadata, independent of any UI.
//! Image strings remain native data; UI boundaries must sanitize them.
mod album;
mod artist;
mod media;
pub use album::AlbumMetadata;
pub use artist::{ArtistCredit, ArtistMetadata};
pub use media::{ConcertDate, ConcertMetadata, VideoCardMetadata, VideoKind};
use serde_json::Value;

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
// JavaScript || preserves empty arrays/objects, unlike an "empty" fallback.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
fn first<'a>(value: &'a Value, fields: &[&str]) -> &'a Value {
    fields
        .iter()
        .map(|field| &value[*field])
        .find(|value| truthy(value))
        .unwrap_or(&Value::Null)
}
