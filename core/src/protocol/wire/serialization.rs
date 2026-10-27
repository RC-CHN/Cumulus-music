//! M/48.f form conversion and M/402 cache ordering. JavaScript numeric/key
//! ordering is preserved explicitly rather than inherited from Rust maps.
use crate::{Error, Result};
use serde_json::{Map, Value};

pub fn is_js_whitespace(c: char) -> bool {
    matches!(c,'\u{9}'..='\u{d}'|'\u{20}'|'\u{a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}

pub fn trim_js(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

pub fn compact_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

pub fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

pub fn js_string(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Number(value) => ryu_js::Buffer::new()
            .format(
                value
                    .as_f64()
                    .ok_or(Error::Protocol("number outside JavaScript range"))?,
            )
            .into(),
        Value::Object(_) => "[object Object]".into(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                if value.is_null() {
                    Ok(String::new())
                } else {
                    js_string(value)
                }
            })
            .collect::<Result<Vec<_>>>()?
            .join(","),
    })
}

fn array_index(key: &str) -> Option<u32> {
    let value = key.parse::<u32>().ok()?;
    (value < u32::MAX && value.to_string() == key).then_some(value)
}

fn js_keys(object: &Map<String, Value>) -> Vec<&str> {
    let mut indices: Vec<_> = object
        .keys()
        .filter_map(|key| array_index(key).map(|index| (index, key.as_str())))
        .collect();
    indices.sort_unstable_by_key(|(index, _)| *index);
    indices
        .into_iter()
        .map(|(_, key)| key)
        .chain(
            object
                .keys()
                .filter(|key| array_index(key).is_none())
                .map(String::as_str),
        )
        .collect()
}

pub fn js_json(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Object(object) => {
            let entries = js_keys(object)
                .into_iter()
                .map(|key| {
                    Ok(format!(
                        "{}:{}",
                        serde_json::to_string(key)?,
                        js_json(&object[key])?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(js_json)
                .collect::<Result<Vec<_>>>()?
                .join(",")
        ),
        Value::Number(_) => js_string(value)?,
        _ => serde_json::to_string(value)?,
    })
}

pub fn encode_component(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for c in text.bytes() {
        if c.is_ascii_alphanumeric() || b"~!*'()-._".contains(&c) {
            out.push(char::from(c))
        } else {
            out.push('%');
            out.push(char::from(HEX[(c >> 4) as usize]));
            out.push(char::from(HEX[(c & 15) as usize]));
        }
    }
    out
}

fn decode_component(text: &str) -> Result<String> {
    let mut out = Vec::new();
    let mut bytes = text.bytes();
    while let Some(c) = bytes.next() {
        if c == b'%' {
            let hi = bytes
                .next()
                .and_then(|c| char::from(c).to_digit(16))
                .ok_or(Error::Protocol("invalid component encoding"))?;
            let lo = bytes
                .next()
                .and_then(|c| char::from(c).to_digit(16))
                .ok_or(Error::Protocol("invalid component encoding"))?;
            out.push((hi * 16 + lo) as u8);
        } else {
            out.push(c)
        }
    }
    String::from_utf8(out).map_err(|_| Error::Protocol("invalid component UTF-8"))
}

fn form_value(value: &Value) -> Result<String> {
    match value {
        Value::Array(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(|v| Ok(encode_component(&js_string(v)?)))
                .collect::<Result<Vec<_>>>()?
                .join(",")
        )),
        Value::Object(_) => Ok(encode_component(&js_json(value)?)),
        _ => Ok(encode_component(&js_string(value)?)),
    }
}

fn form_pair(key: &str, value: &Value) -> Result<String> {
    Ok(format!("{}={}", encode_component(key), form_value(value)?))
}

pub fn official_form(data: &Value) -> Result<String> {
    let object = data
        .as_object()
        .ok_or(Error::InvalidInput("expected form object"))?;
    Ok(js_keys(object)
        .into_iter()
        .map(|key| form_pair(key, &object[key]))
        .collect::<Result<Vec<_>>>()?
        .join("&"))
}

pub fn official_data(data: &Value) -> Result<Value> {
    let mut output = Map::new();
    for pair in official_form(data)?.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && !key.is_empty()
        {
            output.insert(
                decode_component(key)?,
                Value::String(decode_component(value)?),
            );
        }
    }
    Ok(Value::Object(output))
}

pub fn cache_canonical(data: &Value) -> Result<String> {
    let object = data
        .as_object()
        .ok_or(Error::InvalidInput("expected cache object"))?;
    let mut keys: Vec<_> = object
        .keys()
        .filter(|key| key.as_str() != "checktoken" || !js_truthy(&object[*key]))
        .collect();
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(keys
        .into_iter()
        .map(|key| form_pair(key, &object[key]))
        .collect::<Result<Vec<_>>>()?
        .join("&"))
}
