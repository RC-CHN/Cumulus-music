use super::{credits, parse_lrc, parse_yrc, types::*};
use crate::{Error, Result, protocol::wire::serialization::js_truthy};
use serde_json::Value;

fn lyric_text(value: &Value) -> Result<&str> {
    match value.get("lyric") {
        None => Ok(""),
        Some(value) => value
            .as_str()
            .ok_or(Error::Response("lyric must be a string")),
    }
}

fn align(translated: &[LyricLine], original: &[LyricLine]) -> Vec<LyricLine> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(original.len());
    while i < original.len() && j < translated.len() {
        let (a, b) = (&original[i], &translated[j]);
        if a.time == b.time {
            out.push(b.clone());
            i += 1;
            j += 1;
        } else if a.time.zip(b.time).is_some_and(|(a, b)| a > b) {
            j += 1;
        } else {
            out.push(LyricLine {
                time: a.time,
                lyric: String::new(),
            });
            i += 1;
        }
    }
    out.extend(original[i..].iter().map(|line| LyricLine {
        time: line.time,
        lyric: String::new(),
    }));
    out
}

fn add_credits(parsed: &mut ParsedLyrics, values: &[Value], head: bool) -> Result<()> {
    let credits = credits::normalize(values)?;
    for (lines, blank) in [
        (&mut parsed.lyric_lines, false),
        (&mut parsed.tlyric_lines, true),
        (&mut parsed.roma_lyric_lines, true),
    ] {
        if blank && lines.is_empty() {
            continue;
        }
        let added = credits.iter().cloned().map(|mut line| {
            if blank {
                line.lyric.clear();
            }
            line
        });
        if head {
            lines.splice(0..0, added);
        } else {
            lines.extend(added);
        }
    }
    Ok(())
}

pub fn parse_lyrics(response: &Value) -> Result<ParsedLyrics> {
    if !response.is_object() {
        return Err(Error::Response("lyric response must be an object"));
    }
    let mut result = ParsedLyrics::default();
    for key in [
        "askForTranslation",
        "canUploadTranslation",
        "canUploadLyric",
        "lyricContributor",
        "translationContributor",
        "isCloudLyric",
    ] {
        if let Some(value) = response.get(key) {
            result.metadata.insert(key.into(), value.clone());
        }
    }
    let lrc = response.get("lrc").filter(|v| js_truthy(v));
    let yrc = response.get("yrc").filter(|v| js_truthy(v));
    if lrc.is_none() && yrc.is_none() {
        return Ok(result);
    }
    let (mut head, mut tail) = (Vec::new(), Vec::new());
    if let Some(lrc) = lrc {
        let parsed = parse_lrc(lyric_text(lrc)?);
        head = parsed.head_artist_infos;
        tail = parsed.tail_artist_infos;
        result.current_used_lyric = LyricSource::Lrc;
        result.current_used_lyric_version = lrc.get("version").cloned();
        result.offset = match lrc.get("offset").filter(|v| !v.is_null()) {
            Some(value) => value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or(Error::Response("invalid lyric offset"))?,
            None => 0.0,
        };
        result.scrollable = parsed.scrollable;
        result.lyric_lines = parsed.lyric_lines;
        if result.scrollable {
            for (key, out) in [
                ("tlyric", &mut result.tlyric_lines),
                ("romalrc", &mut result.roma_lyric_lines),
            ] {
                if response[key].get("lyric").is_some_and(js_truthy) {
                    let translated = parse_lrc(lyric_text(&response[key])?).lyric_lines;
                    if !translated.is_empty() {
                        *out = align(&translated, &result.lyric_lines);
                    }
                }
            }
        }
    }
    if let Some(yrc) = yrc {
        let parsed = parse_yrc(lyric_text(yrc)?);
        let roma = if response["yromalrc"].get("lyric").is_some_and(js_truthy) {
            lyric_text(&response["yromalrc"])?
        } else if response["romalrc"].get("lyric").is_some_and(js_truthy) {
            lyric_text(&response["romalrc"])?
        } else {
            ""
        };
        let trans = if response["ytlrc"].get("lyric").is_some_and(|v| !v.is_null()) {
            lyric_text(&response["ytlrc"])?
        } else {
            ""
        };
        result.yrc_info = YrcInfo {
            yrc: Some(parsed.raw_yrc),
            yrc_roma: Some(roma.into()),
            yrc_trans: Some(trans.into()),
        };
        if result.current_used_lyric == LyricSource::None {
            head = parsed.head_artist_infos;
            tail = parsed.tail_artist_infos;
            result.current_used_lyric = LyricSource::Yrc;
            result.current_used_lyric_version = yrc.get("version").cloned();
            result.lyric_lines = parsed.lyric_lines;
        }
    }
    let can_upload = response.get("canUploadLyric").is_some_and(js_truthy);
    if (result.lyric_lines.len() == 1
        && result.lyric_lines[0].lyric.contains("纯音乐，请欣赏")
        && head.is_empty()
        && tail.is_empty())
        || (result.lyric_lines.is_empty() && !can_upload)
    {
        result.display_type = DisplayType::Pure;
    }
    if can_upload && result.lyric_lines.is_empty() && result.current_used_lyric == LyricSource::None
    {
        result.display_type = DisplayType::Ask;
    }
    add_credits(&mut result, &head, true)?;
    add_credits(&mut result, &tail, false)?;
    if result.scrollable && result.offset != 0.0 {
        for line in &mut result.lyric_lines {
            if let Some(time) = &mut line.time {
                *time -= result.offset;
                if !time.is_finite() {
                    return Err(Error::Response("lyric offset exceeds finite clock"));
                }
            }
        }
    }
    Ok(result)
}

/// Last timed line at or before the position. At duplicate times, later source
/// lines win. This community helper accepts unsorted input and time zero.
pub fn active_line(lines: &[LyricLine], seconds: f64) -> Option<usize> {
    if !seconds.is_finite() {
        return None;
    }
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            line.time
                .filter(|t| t.is_finite() && *t <= seconds)
                .map(|t| (i, t))
        })
        .max_by(|(ia, a), (ib, b)| a.partial_cmp(b).unwrap().then(ia.cmp(ib)))
        .map(|(i, _)| i)
}
