//! Shared JSON-to-JavaScript numeric coercion for reviewed SDK contracts.
use crate::protocol::wire::serialization::{js_string, trim_js};
use serde_json::Value;

pub(crate) fn js_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Null => 0.,
        Value::Bool(v) => {
            if *v {
                1.
            } else {
                0.
            }
        }
        _ => {
            let Ok(text) = js_string(value) else {
                return f64::NAN;
            };
            let text = trim_js(&text);
            match text {
                "" => return 0.,
                "Infinity" | "+Infinity" => return f64::INFINITY,
                "-Infinity" => return f64::NEG_INFINITY,
                _ => {}
            }
            for (prefix, base) in [
                ("0x", 16),
                ("0X", 16),
                ("0o", 8),
                ("0O", 8),
                ("0b", 2),
                ("0B", 2),
            ] {
                if let Some(digits) = text.strip_prefix(prefix) {
                    if digits.is_empty() {
                        return f64::NAN;
                    }
                    return digits
                        .chars()
                        .try_fold(0., |n, c| {
                            c.to_digit(base).map(|d| n * f64::from(base) + f64::from(d))
                        })
                        .unwrap_or(f64::NAN);
                }
            }
            // Decimal overflow is Infinity in JS too. Restrict the spelling
            // first so Rust-only forms such as `inf` do not become valid.
            if !text
                .bytes()
                .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c))
            {
                return f64::NAN;
            }
            text.parse::<f64>().unwrap_or(f64::NAN)
        }
    }
}
