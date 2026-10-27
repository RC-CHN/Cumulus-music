//! M/15.Hd -> M/12 -> M/402, without the JSK M/290 error conversion.
use crate::{
    Error,
    protocol::api::{Api, ApiFuture},
    protocol::http::MusicHttpClient,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct LegacyValidationApi(pub Arc<MusicHttpClient>);
impl Api for LegacyValidationApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mut value = self.0.request(path, payload, cache).await?;
            if !value.is_object() {
                return Err(Error::Response("legacy validation body must be an object"));
            }
            // M/402 parses a nonempty string code with parseInt(code,10).
            // Invalid numeric strings become NaN (represented as null at this
            // Rust boundary); both are falsy and fail the subscriber's 200 test.
            if value.get("code").is_some() {
                value["code"] = normalized_code(&value["code"])?;
            }
            let code = &value["code"];
            if crate::protocol::wire::serialization::js_truthy(code) && code.as_f64() != Some(200.0)
            {
                return Err(Error::service(path, code.clone(), value));
            }
            Ok(value)
        })
    }
}

fn normalized_code(value: &Value) -> crate::Result<Value> {
    if let Some(text) = value.as_str().filter(|s| !s.is_empty()) {
        let text = text.trim_start_matches(|c: char| {
            matches!(
                c,
                '\t' | '\n'
                    | '\u{000b}'
                    | '\u{000c}'
                    | '\r'
                    | ' '
                    | '\u{00a0}'
                    | '\u{1680}'
                    | '\u{2000}'
                    ..='\u{200a}'
                        | '\u{2028}'
                        | '\u{2029}'
                        | '\u{202f}'
                        | '\u{205f}'
                        | '\u{3000}'
                        | '\u{feff}'
            )
        });
        let digits = text
            .strip_prefix(['+', '-'])
            .unwrap_or(text)
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let length = digits + usize::from(text.starts_with(['+', '-']));
        let code = if digits == 0 {
            Value::Null
        } else {
            let number = text[..length]
                .parse::<f64>()
                .map_err(|_| Error::Response("invalid legacy validation code"))?;
            if !number.is_finite() {
                return Err(Error::Response("nonfinite legacy validation code"));
            }
            if number == 200.0 {
                json!(200)
            } else {
                json!(number)
            }
        };
        return Ok(code);
    }

    Ok(value.clone())
}

pub(super) fn intercepted_code(value: &Value) -> bool {
    normalized_code(value).is_ok_and(|v| v.as_f64() == Some(301.0))
}
