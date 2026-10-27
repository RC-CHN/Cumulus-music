use super::{
    credits,
    types::{LyricLine, ParsedYrc},
};
use crate::protocol::wire::serialization::{is_js_whitespace, trim_js};
use regex::Regex;
use std::sync::LazyLock;

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(([0-9]+),([0-9]+),[0-9]+\)").unwrap());
/// Word rendering companion for the recovered YRC absolute-millisecond format.
/// Invalid timing lines fall back to the existing plain line renderer.
pub fn parse_yrc_words(text: &str) -> Vec<super::types::WordLine> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let Some(header) = HEADER.captures(line) else {
            continue;
        };
        let number = |v: &str| {
            v.parse::<u64>()
                .ok()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .map(|n| n as f64 / 1000.0)
        };
        let (Some(time), Some(duration)) = (number(&header[1]), number(&header[2])) else {
            continue;
        };
        let content = &line[header.get(0).unwrap().end()..];
        let markers = WORD.captures_iter(content).collect::<Vec<_>>();
        let words = markers
            .iter()
            .enumerate()
            .map(|(index, marker)| {
                let start = number(&marker[1])?;
                let length = number(&marker[2])?;
                if start < time || start + length > time + duration + 0.001 {
                    return None;
                }
                let end = markers
                    .get(index + 1)
                    .map_or(content.len(), |m| m.get(0).unwrap().start());
                Some(super::types::TimedWord {
                    time: start,
                    duration: length,
                    text: displayed_fragment(&content[marker.get(0).unwrap().end()..end]),
                })
            })
            .collect::<Option<Vec<_>>>();
        // An untimed prefix has no defensible word clock. Keep the complete
        // line fallback instead of silently dropping text from the renderer.
        let prefix = markers
            .first()
            .map_or(content, |m| &content[..m.get(0).unwrap().start()]);
        let words = if trim_js(prefix).is_empty() {
            words.unwrap_or_default()
        } else {
            Vec::new()
        };
        lines.push(super::types::WordLine {
            time,
            duration,
            words,
            text: parse_yrc(line)
                .lyric_lines
                .into_iter()
                .next()
                .map(|l| l.lyric)
                .unwrap_or_default(),
        });
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

static HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([0-9]+),([0-9]+)\]").unwrap());
static WORD_TIMES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\([^\r\n\x{2028}\x{2029}]*?\)").unwrap());

// M/370 inserts separators for alphabetic fragments. Use the same rule for
// individual timed words so English does not concatenate when word mode is on.
fn displayed_fragment(text: &str) -> String {
    let mut result = String::new();
    for word in trim_js(text)
        .split(is_js_whitespace)
        .filter(|s| !s.is_empty())
    {
        result.push_str(word);
        if word.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && word.as_bytes().last().is_some_and(u8::is_ascii_alphabetic)
        {
            result.push(' ');
        }
    }
    result
}

/// Official line fallback; raw word timing text is retained for a word renderer.
pub fn parse_yrc(text: &str) -> ParsedYrc {
    let sections = credits::split(text, true);
    let raw_yrc = sections
        .lines
        .iter()
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let mut lyric_lines = Vec::new();
    for line in sections.lines {
        let Some(header) = HEADER.captures(line) else {
            continue;
        };
        let Some(ms) = header[1].parse::<f64>().ok().filter(|v| v.is_finite()) else {
            continue;
        };
        let words = WORD_TIMES.replace_all(&line[header.get(0).unwrap().end()..], " ");
        let lyric = displayed_fragment(&words);
        lyric_lines.push(LyricLine {
            time: Some(ms / 1000.0),
            lyric,
        });
    }
    lyric_lines.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    ParsedYrc {
        head_artist_infos: sections.head,
        tail_artist_infos: sections.tail,
        lyric_lines,
        raw_yrc,
    }
}
