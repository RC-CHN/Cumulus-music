//! Text projection for the two recovered PC comment tags. Never emit markup.
use crate::protocol::wire::serialization::decode_component;
use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::LazyLock;

const MAX_TEXT: usize = 16 * 1024;

static SONG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<song\s+id="[^"]*">([^<]*)</song>"#).unwrap());
static EMOJI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<c0m_cvt(?:\s+[^>]*)?>").unwrap());
static EMOJI_JSON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\sj="([^"]+)""#).unwrap());

fn decode_emoji(tag: &str) -> String {
    EMOJI_JSON
        .captures(tag)
        .and_then(|parts| decode_component(parts.get(1)?.as_str()).ok())
        .and_then(|decoded| serde_json::from_str::<Value>(&decoded).ok())
        .and_then(|value| value["text"].as_str().map(str::to_owned))
        .filter(|text| text.chars().count() <= 128 && !text.contains(['<', '>']))
        .unwrap_or_else(|| "[表情]".into())
}

fn render(source: &str) -> String {
    let songs = SONG.replace_all(source, "${1} ");
    EMOJI
        .replace_all(&songs, |parts: &Captures<'_>| decode_emoji(&parts[0]))
        .replace("</c0m_cvt>", "")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}

pub(super) fn project(rich: &Value, plain: Option<&str>, status: Option<i64>) -> Option<String> {
    if let Some(source) = rich
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= MAX_TEXT)
    {
        let rendered = render(source);
        // Unknown rich markup may contain private parameters, so use the
        // ordinary text field instead of forwarding the raw rich payload.
        if !rendered.contains(['<', '>']) && !rendered.is_empty() {
            return Some(rendered);
        }
    }
    if let Some(source) = plain.filter(|text| !text.is_empty()) {
        let end = source
            .char_indices()
            .map(|(i, _)| i)
            .find(|&i| i >= MAX_TEXT)
            .unwrap_or(source.len());
        return Some(render(&source[..end]));
    }
    (status == Some(-10)).then(|| "该评论涉及违规内容，经举报已被屏蔽".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn projects_known_tags_without_exporting_attributes() {
        let payload = crate::protocol::wire::serialization::encode_component(
            r#"{"text":"[大笑]+","url":"private marker"}"#,
        );
        let rich = json!(format!(
            r#"听过<song id="track-private">合成曲</song><c0m_cvt j="{payload}" href="another-private"/>"#
        ));
        assert_eq!(
            project(&rich, Some("旧文本"), None).as_deref(),
            Some("听过合成曲 [大笑]+")
        );
        assert_eq!(
            project(&json!(r#"<c0m_cvt j="%zz"/>"#), None, None).as_deref(),
            Some("[表情]")
        );
    }

    #[test]
    fn falls_back_for_unknown_or_oversize_rich_input_and_preserves_plain_text() {
        assert_eq!(
            project(
                &json!(r#"<unknown href="private">内容</unknown>"#),
                Some("原文 <3\n"),
                None
            )
            .as_deref(),
            Some("原文 <3\n")
        );
        assert_eq!(
            project(&json!("x".repeat(MAX_TEXT + 1)), Some("原文"), None).as_deref(),
            Some("原文")
        );
        assert_eq!(
            project(&Value::Null, None, Some(-10)).as_deref(),
            Some("该评论涉及违规内容，经举报已被屏蔽")
        );
    }
}
