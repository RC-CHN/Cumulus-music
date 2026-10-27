use crate::{
    Error, Result,
    account::credentials::LoginCredentials,
    protocol::{
        api::identifier,
        wire::{
            eapi::md5_hex,
            serialization::{encode_component, js_json, js_string, trim_js},
        },
    },
};
use serde_json::{Value, json};

#[derive(Clone, Default)]
pub struct DeviceInfo {
    pub device_name: String,
    pub device_display_name: String,
    pub device_id: String,
    pub os: String,
    pub client_type: Option<String>,
}

/// M/1368 pn(), for valid Unicode scalar strings accepted by the Rust host.
pub fn encoded_model(value: Option<&str>) -> String {
    let Some(value) = value.filter(|s| !trim_js(s).is_empty()) else {
        return "unknown".into();
    };
    encode_component(&value.chars().filter(|c| *c > '\u{20}').collect::<String>())
}

/// M/1368 encryptParams: JS UTF-16 key order and JS object/number serialization.
pub fn encrypt_params(values: &Value) -> Result<String> {
    let fields = values.as_object().ok_or(Error::InvalidInput(
        "refresh signature fields must be an object",
    ))?;
    let mut keys: Vec<_> = fields.keys().collect();
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let mut text = String::new();
    for key in keys {
        let value = &fields[key];
        let value = match value {
            Value::Null => String::new(),
            Value::Array(_) | Value::Object(_) => js_json(value)?,
            _ => js_string(value)?,
        };
        text.push_str(key);
        text.push('=');
        text.push_str(&value);
        text.push('&');
    }
    text.push_str("_scver=1&_sckey=7ada0f7ccadbe165e6e7fbe01113f4df");
    Ok(md5_hex(text.as_bytes()))
}

pub fn refresh_payload(
    credentials: &LoginCredentials,
    uid: &Value,
    device: &DeviceInfo,
    yd_device_token: &str,
) -> Result<Value> {
    let token = credentials
        .token
        .as_deref()
        .filter(|v| !v.is_empty())
        .ok_or(Error::State("session has no MUSIC_U"))?;
    let refresh = credentials
        .refresh_token
        .as_deref()
        .filter(|v| !v.is_empty())
        .ok_or(Error::State("session has no refresh token"))?;
    // M/290's desktop bridge returns no token. M/1368 resolves its event
    // timeout to "" and forwards that value; it is not a fabricated token.
    if yd_device_token.len() > 16384 || yd_device_token.contains(['\r', '\n', '\0']) {
        return Err(Error::InvalidInput("invalid ydDeviceToken"));
    }
    let name = device.device_name.replacen(',', ".", 1);
    let model = encoded_model(Some(if name.is_empty() {
        &device.device_display_name
    } else {
        &name
    }));
    let os = device
        .client_type
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&device.os);
    let uid = identifier(uid)?;
    let sign = encrypt_params(
        &json!({"deviceId":device.device_id,"model":model,"os":os,"refreshToken":refresh,"token":token,"userId":uid}),
    )?;
    Ok(
        json!({"refreshToken":refresh,"clientUserId":uid,"ydDeviceToken":yd_device_token,"_scver":"1","_sign":sign}),
    )
}
