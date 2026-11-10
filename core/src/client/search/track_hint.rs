//! M/21 song tags and M/123 marks for the separate card and table surfaces.
use crate::{catalog::search, protocol::wire::serialization::js_truthy};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackHint {
    Lyric { parts: Vec<LyricPart> },
    Alias { text: String },
}

#[derive(Clone, Serialize)]
pub struct LyricPart {
    text: String,
    highlight: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongTableHint {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    alias_parts: Vec<search::SearchSuggestionPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lyric_parts: Option<Vec<LyricPart>>,
}

pub(super) fn song_table(base: &Value, resource: &Value, keyword: &str) -> Option<SongTableHint> {
    // M/15.Un first spreads the resource and its simpleSongData, then
    // unconditionally replaces lyrics with the original resource extInfo.
    // M/21.x then merges newSongExt and extInfo and uses lyrics || lyric.
    let merged = |key: &str| base.get(key).unwrap_or(&resource[key]);
    let tag = merged("extInfo")
        .get("songAlias")
        .unwrap_or(&merged("newSongExt")["songAlias"]);
    let aliases = if js_truthy(merged("alia")) {
        merged("alia")
    } else {
        merged("ala")
    };
    let alias_parts = if js_truthy(tag) {
        aliases
            .as_array()
            .filter(|rows| rows.len() <= 16)
            .and_then(|rows| {
                rows.iter()
                    .map(|row| {
                        row.as_str()
                            .filter(|text| text.is_empty() || safe_text(text))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .map(|aliases| search::song_table_alias_parts(&aliases, keyword))
            .unwrap_or_default()
    } else {
        vec![]
    };
    let lyrics = &resource["extInfo"]["lyrics"];
    let lyric_parts = lyric(if js_truthy(lyrics) {
        lyrics
    } else {
        merged("lyric")
    });
    (!alias_parts.is_empty() || lyric_parts.is_some()).then_some(SongTableHint {
        alias_parts,
        lyric_parts,
    })
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
