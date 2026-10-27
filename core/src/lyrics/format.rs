use super::LyricLine;
use crate::{Error, Result, protocol::wire::serialization::trim_js};
use regex::Regex;
use std::sync::LazyLock;

const JS_SPACE: &str = r"[\x{9}-\x{d}\x{20}\x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]";
static TIMESTAMP_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:\[[0-9]{{1,3}}:[0-9]{{1,2}}(?:[.:][0-9]{{1,3}})?\]{JS_SPACE}*)+$|^\[[0-9]+,[0-9]+\]{JS_SPACE}*$")).unwrap()
});
static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r\n|\r|\n").unwrap());

/// M/370.d truncates the JS decimal string; it does not round to milliseconds.
pub fn format_timestamp(seconds: f64) -> Result<String> {
    if !seconds.is_finite() {
        return Err(Error::InvalidInput("lyric time must be finite"));
    }
    let minute = ryu_js::Buffer::new()
        .format((seconds / 60.0).floor())
        .to_string();
    let remainder = ryu_js::Buffer::new().format(seconds % 60.0).to_string();
    let (whole, fraction) = remainder.split_once('.').unwrap_or((&remainder, "0"));
    let fraction = &fraction[..fraction.len().min(3)];
    Ok(format!("{minute:0>2}:{whole:0>2}.{fraction}"))
}

pub fn render_lrc(lines: &[LyricLine]) -> Result<String> {
    lines
        .iter()
        .map(|line| match line.time {
            Some(time) => Ok(format!("[{}]{}", format_timestamp(time)?, line.lyric)),
            None => Ok(line.lyric.clone()),
        })
        .collect::<Result<Vec<_>>>()
        .map(|lines| lines.join("\n"))
}

/// Keep original line endings and whitespace, adding a zero-width space only
/// to otherwise empty, recognized timestamp lines (M/370.b).
pub fn protect_timestamp_only_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    for end in NEWLINES.find_iter(text) {
        append_line(&mut out, &text[start..end.start()]);
        out.push_str(end.as_str());
        start = end.end();
    }
    append_line(&mut out, &text[start..]);
    out
}

fn append_line(out: &mut String, line: &str) {
    out.push_str(line);
    if TIMESTAMP_ONLY.is_match(trim_js(line)) {
        out.push('\u{200b}');
    }
}
