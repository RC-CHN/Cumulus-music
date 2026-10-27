use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy},
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum FieldKind {
    Number,
    Boolean,
    String,
    Hex,
    Array,
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum FieldLimit {
    Scalar(usize),
    Elements(Vec<usize>),
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Descriptor {
    pub key: String,
    pub id: u16,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub limit: FieldLimit,
}
pub fn descriptors() -> Result<Vec<Descriptor>> {
    Ok(serde_json::from_str(include_str!("fields.json"))?)
}
fn number(value: f64, limit: Option<usize>) -> Vec<u8> {
    // JS ToUint32, followed by ea(W(v), n); n>4 still yields four bytes.
    let n = if value.is_finite() {
        value.trunc().rem_euclid(4294967296.0) as u32
    } else {
        0
    };
    let bytes = n.to_be_bytes();
    bytes[4 - limit.filter(|n| *n != 0).unwrap_or(4).min(4)..].to_vec()
}
fn text(value: &str, limit: Option<usize>) -> Result<Vec<u8>> {
    let units: Vec<_> = value
        .encode_utf16()
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    // Official encodeURIComponent throws on a cut surrogate. Do not silently
    // substitute U+FFFD or change the field's UTF-16 truncation boundary.
    String::from_utf16(&units)
        .map(String::into_bytes)
        .map_err(|_| Error::Protocol("Watchman field truncation splits a surrogate"))
}
pub fn encode_field(field: &Descriptor, value: &Value, collection_error: bool) -> Result<Vec<u8>> {
    let data = if collection_error {
        Vec::new()
    } else {
        match (&field.kind, &field.limit) {
            (FieldKind::Boolean, FieldLimit::Scalar(n)) => {
                number(if js_truthy(value) { 1.0 } else { 2.0 }, Some(*n))
            }
            (FieldKind::Number, FieldLimit::Scalar(n)) => {
                number(super::value::js_number(value), Some(*n))
            }
            (FieldKind::String, FieldLimit::Scalar(n)) => match value {
                Value::String(value) => text(value, Some(*n))?,
                Value::Array(values) => {
                    // ga() keeps the last N array entries, then encodeURIComponent
                    // coerces the array. It does not truncate the joined string.
                    let start = if *n == 0 {
                        0
                    } else {
                        values.len().saturating_sub(*n)
                    };
                    js_string(&Value::Array(values[start..].to_vec()))?.into_bytes()
                }
                Value::Null => return Err(Error::InvalidInput("Watchman null text field")),
                value => js_string(value)?.into_bytes(),
            },
            (FieldKind::Hex, FieldLimit::Scalar(n)) => {
                let bytes = hex::decode(
                    value
                        .as_str()
                        .ok_or(Error::InvalidInput("Watchman hex field requires text"))?,
                )
                .map_err(|_| Error::InvalidInput("invalid Watchman field hex"))?;
                bytes[if *n == 0 {
                    0
                } else {
                    bytes.len().saturating_sub(*n)
                }..]
                    .to_vec()
            }
            (FieldKind::Array, FieldLimit::Elements(limits)) => {
                let values = value.as_array().ok_or(Error::InvalidInput(
                    "Watchman array field requires an array",
                ))?;
                if values.len() > 4096 {
                    return Err(Error::InvalidInput("Watchman array exceeds limit"));
                }
                let mut out = Vec::new();
                for (i, v) in values.iter().enumerate() {
                    if let Some(n) = v.as_f64() {
                        out.extend(number(n, limits.get(i).copied()));
                    } else if let Some(s) = v.as_str() {
                        out.extend(text(s, limits.get(i).copied())?);
                    }
                    if out.len() > u16::MAX as usize {
                        return Err(Error::InvalidInput("Watchman field exceeds length header"));
                    }
                }
                out
            }
            _ => return Err(Error::InvalidInput("Watchman field descriptor mismatch")),
        }
    };
    let length = u16::try_from(data.len())
        .map_err(|_| Error::InvalidInput("Watchman field exceeds length header"))?;
    Ok([
        field.id.to_be_bytes().as_slice(),
        length.to_be_bytes().as_slice(),
        &data,
    ]
    .concat())
}
/// The caller supplies field order (the SDK shuffles collection groups). Repeated
/// event IDs are valid; unknown keys fail instead of silently omitting facts.
pub fn encode_fields(values: &[(&str, Value, bool)]) -> Result<Vec<u8>> {
    let fields = descriptors()?;
    let mut out = Vec::new();
    for (key, value, failed) in values {
        let field = fields
            .iter()
            .find(|f| f.key == *key)
            .ok_or(Error::InvalidInput("unknown Watchman field"))?;
        out.extend(encode_field(field, value, *failed)?);
        if out.len() > super::MAX_BYTES {
            return Err(Error::InvalidInput("Watchman fields exceed packet limit"));
        }
    }
    Ok(out)
}
