use super::encode_text;
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_json, js_string},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use serde_json::{Value, json};

/// Native storage semantics for pure dc/kb compatibility. No Debug/Serialize:
/// identifiers and generated tokens must not enter frontend DTOs or logs.
pub enum StoredValue<'a> {
    Undefined,
    Null,
    Text(&'a str),
}
fn check_ret(ret: &Value) -> Result<()> {
    if !ret.is_null() && !ret.is_boolean() && !ret.is_number() && !ret.is_string() {
        return Err(Error::InvalidInput("Watchman ret must be a primitive"));
    }
    Ok(())
}
fn did_text<'a>(did: &'a StoredValue<'a>) -> &'a str {
    match did {
        StoredValue::Text(s) => s,
        _ => "",
    }
}
pub fn encode_token(
    ret: &Value,
    did: StoredValue<'_>,
    nonce: &str,
    fallback: bool,
) -> Result<String> {
    check_ret(ret)?;
    let mut value = json!({"r":ret,"d":did_text(&did),"b":nonce});
    if fallback {
        // Preserve JS left-associative +: number+null differs from number+"";
        // number+undefined becomes NaN before nonce makes it a string.
        let prefix = match &did {
            StoredValue::Text(s) => format!("{}{s}", js_string(ret)?),
            StoredValue::Null if ret.is_string() => format!("{}null", js_string(ret)?),
            StoredValue::Undefined if ret.is_string() => format!("{}undefined", js_string(ret)?),
            StoredValue::Undefined => "NaN".into(),
            StoredValue::Null => match ret {
                Value::Null => "0".into(),
                Value::Bool(b) => if *b { "1" } else { "0" }.into(),
                _ => js_string(ret)?,
            },
        };
        let digest = Md5::digest(format!("{prefix}{nonce}WoeTpXnDDPhiAvsJUUIY3RdAo2PKaVwi"));
        value["t"] = STANDARD.encode(digest).into();
    }
    encode_text(&js_json(&value)?)
}
pub fn encode_nd_info(ret: &Value, did: &str, ni: StoredValue<'_>) -> Result<String> {
    check_ret(ret)?;
    let mut value = json!({"r":ret,"d":did});
    match ni {
        StoredValue::Undefined => {}
        StoredValue::Null => value["i"] = Value::Null,
        StoredValue::Text(s) => value["i"] = s.into(),
    }
    encode_text(&js_json(&value)?)
}
