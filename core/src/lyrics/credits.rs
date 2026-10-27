use super::types::LyricLine;
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy, trim_js},
};
use regex::Regex;
use serde_json::Value;
use std::{collections::VecDeque, sync::LazyLock};

static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r\n|\r|\n").unwrap());

pub(super) struct Sections<'a> {
    pub lines: VecDeque<&'a str>,
    pub head: Vec<Value>,
    pub tail: Vec<Value>,
}

pub(super) fn split(text: &str, yrc: bool) -> Sections<'_> {
    let mut lines: VecDeque<_> = NEWLINES.split(trim_js(text)).map(trim_js).collect();
    let mut head = Vec::new();
    let mut tail = Vec::new();
    while let Some(line) = lines.pop_front() {
        if line.is_empty() {
            break;
        }
        match serde_json::from_str(line) {
            Ok(value) => head.push(value),
            Err(_) => {
                lines.push_front(line);
                break;
            }
        }
    }
    while let Some(line) = lines.pop_back() {
        if line.is_empty() {
            break;
        }
        match serde_json::from_str(line) {
            Ok(value) => tail.push(value),
            Err(_) => {
                lines.push_back(line);
                break;
            }
        }
    }
    // M/370 deliberately reverses YRC tail credits, but preserves LRC order.
    if !yrc {
        tail.reverse();
    }
    Sections { lines, head, tail }
}

pub(super) fn normalize(values: &[Value]) -> Result<Vec<LyricLine>> {
    values
        .iter()
        .map(|credit| {
            let parts = credit
                .get("c")
                .and_then(Value::as_array)
                .filter(|v| !v.is_empty())
                .ok_or(Error::Response("invalid lyric credit"))?;
            if !parts[0].is_object() {
                return Err(Error::Response("invalid first lyric credit part"));
            }
            let mut lyric = parts[0]
                .get("tx")
                .map(js_string)
                .transpose()?
                .unwrap_or_else(|| "undefined".into());
            let remaining = parts[1..]
                .iter()
                .filter_map(|p| p.get("tx"))
                .filter(|p| js_truthy(p))
                .map(js_string)
                .collect::<Result<Vec<_>>>()?
                .join(" ");
            lyric.push_str(&remaining);
            let time = match credit.get("t") {
                Some(v) if js_truthy(v) || v.as_f64() == Some(0.0) => Some(
                    v.as_f64()
                        .filter(|t| t.is_finite())
                        .ok_or(Error::Response("invalid lyric credit time"))?
                        / 1000.0,
                ),
                _ => None,
            };
            Ok(LyricLine { time, lyric })
        })
        .collect()
}
