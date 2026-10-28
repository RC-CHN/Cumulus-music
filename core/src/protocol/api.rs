//! Transport-independent recovered API contracts. A concrete HTTP transport
//! will implement `Api`; this layer never chooses a new protocol after failure.
use crate::{Error, Result, error::code_matches};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin};

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait Api: Send + Sync + std::any::Any {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value>;

    /// Native transport identity used by device-scoped service settings. Never
    /// accepted from IPC payloads; adapters without a bound identity are read-only.
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn call<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        cache: bool,
        accepted_codes: &'a [i64],
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let value = self.request(path, payload, cache).await?;
            validate_response(path, value, accepted_codes)
        })
    }

    fn account(&self) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            const PATH: &str = "/api/w/nuser/account/get";
            let response = self.call(PATH, json!({}), false, &[200]).await?;
            match response.get("profile") {
                Some(profile) if profile.is_object() && identifier(&profile["userId"]).is_ok() => {
                    Ok(profile.clone())
                }
                _ => Err(Error::service(PATH, json!(301), response)),
            }
        })
    }
}

pub fn validate_response(path: &str, value: Value, accepted_codes: &[i64]) -> Result<Value> {
    if !value.is_object() {
        return Err(Error::Response("API response must be an object"));
    }
    let code = &value["code"];
    if !accepted_codes
        .iter()
        .any(|expected| code_matches(code, *expected))
    {
        return Err(Error::service(path, code.clone(), value));
    }
    Ok(value)
}

pub fn identifier(value: &Value) -> Result<String> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => {
            return Err(Error::InvalidInput(
                "expected a positive decimal identifier",
            ));
        }
    };
    if text.is_empty()
        || !text.bytes().all(|c| c.is_ascii_digit())
        || !text.bytes().any(|c| c != b'0')
    {
        return Err(Error::InvalidInput(
            "expected a positive decimal identifier",
        ));
    }
    // No u64 narrowing: official IDs are decimal strings, including leading
    // zeros and values larger than JavaScript's exact integer range.
    Ok(text)
}

pub fn packed_ids(values: &[Value]) -> Result<String> {
    Ok(serde_json::to_string(
        &values.iter().map(identifier).collect::<Result<Vec<_>>>()?,
    )?)
}

pub fn official_text_length(value: &str) -> usize {
    value
        .encode_utf16()
        .map(|unit| if unit <= 255 { 1 } else { 2 })
        .sum()
}

pub fn playlist_name(value: &str) -> Result<String> {
    // Python's recovered validator also strips the four ASCII information
    // separators, unlike Rust str::trim. Preserve that accepted-input boundary.
    let value =
        value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
    if value.is_empty() || value.contains(['@', '#']) || official_text_length(value) > 40 {
        return Err(Error::InvalidInput(
            "invalid playlist name or weighted length exceeds 40",
        ));
    }
    Ok(value.into())
}
