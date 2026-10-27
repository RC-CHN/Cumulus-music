use super::{
    credits,
    types::{LyricLine, ParsedLrc},
};
use crate::protocol::wire::serialization::is_js_whitespace;
use regex::Regex;
use std::sync::LazyLock;

static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\r\n\x{2028}\x{2029}]*?)\]").unwrap());
static INT_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?[0-9]+").unwrap());
static FLOAT_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[+-]?(?:Infinity|(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)").unwrap()
});

fn timestamp(text: &str) -> Option<f64> {
    let mut parts = text.split(':');
    let minute = parts.next()?.trim_start_matches(is_js_whitespace);
    let minute: f64 = INT_PREFIX.find(minute)?.as_str().parse().ok()?;
    let seconds = parts.collect::<Vec<_>>().join(".");
    let second: f64 = FLOAT_PREFIX
        .find(seconds.trim_start_matches(is_js_whitespace))?
        .as_str()
        .parse()
        .ok()?;
    let time = minute * 60.0 + second;
    time.is_finite().then_some(time)
}

pub fn parse_lrc(text: &str) -> ParsedLrc {
    let sections = credits::split(text, false);
    let mut lyric_lines = Vec::new();
    for line in sections.lines {
        let words = TAG.replace_all(line, "");
        if words.is_empty() {
            continue;
        }
        let mut tagged = false;
        for capture in TAG.captures_iter(line) {
            tagged = true;
            if let Some(time) = timestamp(&capture[1]) {
                lyric_lines.push(LyricLine {
                    time: Some(time),
                    lyric: words.to_string(),
                });
            }
        }
        if !tagged {
            lyric_lines.push(LyricLine {
                time: None,
                lyric: words.into_owned(),
            });
        }
    }
    let scrollable = !lyric_lines.is_empty() && lyric_lines.iter().all(|line| line.time.is_some());
    if scrollable {
        lyric_lines.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }
    ParsedLrc {
        head_artist_infos: sections.head,
        tail_artist_infos: sections.tail,
        lyric_lines,
        scrollable,
    }
}
