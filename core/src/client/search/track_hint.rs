//! M/21 song tags and the M/123 one-line lyric match used by TrackCard.
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackHint {
    Lyric { parts: Vec<LyricPart> },
    Alias { text: String },
}

#[derive(Serialize)]
pub struct LyricPart {
    text: String,
    highlight: bool,
}

pub(super) fn project(base: &Value, resource: &Value) -> Option<TrackHint> {
    // The comprehensive adapters flatten resource/base fields, then M/21
    // merges newSongExt and extInfo; a supplied extInfo field wins per key.
    let tag = |key: &str| {
        [
            &resource["extInfo"],
            &base["extInfo"],
            &base["newSongExt"],
            &resource["newSongExt"],
        ]
        .into_iter()
        .find_map(|record| record.get(key))
    };
    if let Some(parts) = tag("lyrics").and_then(lyric) {
        return Some(TrackHint::Lyric { parts });
    }
    let raw = tag("songAlias")?.as_str().filter(|raw| raw.len() <= 1024)?;
    let text = super::results::clean(raw);
    let text = text.trim();
    safe_text(text).then(|| TrackHint::Alias { text: text.into() })
}

fn safe_text(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 1024
        && text.encode_utf16().count() <= 256
        && !text.contains("://")
        && !text.chars().any(char::is_control)
}

fn lyric(data: &Value) -> Option<Vec<LyricPart>> {
    let text = data["txt"].as_str().filter(|text| safe_text(text))?;
    let ranges = data["range"].as_array().filter(|rows| rows.len() <= 16)?;
    // JS slices at UTF-16 code units. Map valid character boundaries back to
    // UTF-8 and reject malformed spans rather than highlighting another word.
    let mut boundaries = vec![Some(0)];
    for (byte, ch) in text.char_indices() {
        if ch.len_utf16() == 2 {
            boundaries.push(None);
        }
        boundaries.push(Some(byte + ch.len_utf8()));
    }
    let position = |value: &Value| {
        let index = usize::try_from(value.as_u64()?).ok()?;
        boundaries.get(index).copied().flatten()
    };
    let mut parts = Vec::new();
    let mut last = 0;
    for row in ranges {
        let first = position(&row["first"])?;
        let end = position(&row["second"])?;
        if first < last || first >= end {
            return None;
        }
        if first > last {
            parts.push(LyricPart {
                text: text[last..first].into(),
                highlight: false,
            });
        }
        parts.push(LyricPart {
            text: text[first..end].into(),
            highlight: true,
        });
        last = end;
    }
    if last < text.len() {
        parts.push(LyricPart {
            text: text[last..].into(),
            highlight: false,
        });
    }
    Some(parts)
}
