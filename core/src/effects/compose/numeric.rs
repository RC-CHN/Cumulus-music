//! JSON subset of JS ToPrimitive/ToNumber/addition used by recovered rules.
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, trim_js},
};
use serde_json::{Number, Value};

enum Primitive {
    Number(f64),
    String(String),
}
fn primitive(value: &Value) -> Result<Primitive> {
    Ok(match value {
        Value::Null => Primitive::Number(0.0),
        Value::Bool(value) => Primitive::Number(f64::from(u8::from(*value))),
        Value::Number(value) => Primitive::Number(
            value
                .as_f64()
                .ok_or(Error::InvalidInput("invalid composition number"))?,
        ),
        _ => Primitive::String(js_string(value)?),
    })
}
fn string_number(text: &str) -> f64 {
    let text = trim_js(text);
    if text.is_empty() {
        return 0.0;
    }
    if matches!(text, "Infinity" | "+Infinity") {
        return f64::INFINITY;
    }
    if text == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() {
                return f64::NAN;
            }
            let mut result = 0.0;
            for ch in digits.chars() {
                let Some(value) = ch.to_digit(radix) else {
                    return f64::NAN;
                };
                result = result * f64::from(radix) + f64::from(value);
            }
            return result;
        }
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}
fn as_number(value: &Primitive) -> f64 {
    match value {
        Primitive::Number(value) => *value,
        Primitive::String(value) => string_number(value),
    }
}
fn as_string(value: &Primitive) -> String {
    match value {
        Primitive::String(value) => value.clone(),
        Primitive::Number(value) => ryu_js::Buffer::new().format(*value).to_owned(),
    }
}
fn add(left: Primitive, right: Primitive) -> Primitive {
    if matches!(left, Primitive::String(_)) || matches!(right, Primitive::String(_)) {
        Primitive::String(as_string(&left) + &as_string(&right))
    } else {
        Primitive::Number(as_number(&left) + as_number(&right))
    }
}
pub(super) fn number(value: &Value) -> Result<f64> {
    Ok(as_number(&primitive(value)?))
}
pub(super) fn average(values: [&Value; 3]) -> Result<f64> {
    Ok(as_number(&add(
        add(primitive(values[0])?, primitive(values[1])?),
        primitive(values[2])?,
    )) / 3.0)
}
pub(super) fn sum(left: &Value, right: &Value) -> Result<Value> {
    match add(primitive(left)?, primitive(right)?) {
        Primitive::String(value) => Ok(Value::String(value)),
        Primitive::Number(value) => finite(value),
    }
}
pub(super) fn finite(value: f64) -> Result<Value> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or(Error::InvalidInput("non-finite composed parameter"))
}
