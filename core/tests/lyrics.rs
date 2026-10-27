use cumulus_core::{Error, lyrics::*};
use serde_json::{Value, json};

fn assert_json(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{actual} != {expected}");
            for (key, value) in b {
                assert!(a.contains_key(key), "missing {key}");
                assert_json(&a[key], value);
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_json(a, b);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn all_four_exports_match_executed_official_module() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/lyrics.json")).unwrap();
    for row in fixture["parsed"].as_array().unwrap() {
        let parsed = parse_lyrics(&row["input"]).unwrap();
        assert_json(&serde_json::to_value(parsed).unwrap(), &row["expected"]);
    }
    for row in fixture["normalized"].as_array().unwrap() {
        let actual = if row["kind"] == "song" {
            normalize_song_response(&row["input"])
        } else {
            normalize_cloud_response(&row["input"])
        }
        .unwrap();
        assert_json(&actual, &row["expected"]);
    }
    for row in fixture["protected"].as_array().unwrap() {
        assert_eq!(
            protect_timestamp_only_lines(row["input"].as_str().unwrap()),
            row["expected"]
        );
    }
    for row in fixture["rejected"].as_array().unwrap() {
        assert!(parse_lyrics(&row["input"]).is_err());
    }
    for row in fixture["times"].as_array().unwrap() {
        assert_eq!(
            format_timestamp(row["input"].as_f64().unwrap()).unwrap(),
            row["expected"]
        );
    }
    for row in fixture["rendered"].as_array().unwrap() {
        let lines: Vec<LyricLine> = serde_json::from_value(row["input"].clone()).unwrap();
        assert_eq!(render_lrc(&lines).unwrap(), row["expected"]);
    }
}

#[test]
fn active_line_handles_offset_zero_duplicates_unsorted_and_invalid_clocks() {
    let parsed = parse_lyrics(
        &json!({"lrc":{"lyric":"[00:02]two\n[00:01]one\n[00:01]duplicate","offset":1}}),
    )
    .unwrap();
    assert_eq!(active_line(&parsed.lyric_lines, -0.01), None);
    assert_eq!(active_line(&parsed.lyric_lines, 0.0), Some(1));
    assert_eq!(active_line(&parsed.lyric_lines, 1.0), Some(2));
    let lines = vec![
        LyricLine {
            time: Some(5.0),
            lyric: "last".into(),
        },
        LyricLine {
            time: None,
            lyric: "credit".into(),
        },
        LyricLine {
            time: Some(f64::NAN),
            lyric: "broken".into(),
        },
        LyricLine {
            time: Some(0.0),
            lyric: "first".into(),
        },
    ];
    assert_eq!(active_line(&lines, 0.0), Some(3));
    assert_eq!(active_line(&lines, 10.0), Some(0));
    assert_eq!(active_line(&lines, f64::INFINITY), None);
    assert_eq!(active_line(&lines, f64::NAN), None);
}

#[test]
fn finite_clock_boundary_rejects_bad_structure_and_drops_invalid_stamps() {
    let parsed = parse_lrc("[00:Infinity]bad\n[00:1e999]overflow\n[ar:unknown]metadata\n[00:01]ok");
    assert_eq!(parsed.lyric_lines.len(), 1);
    assert_eq!(parsed.lyric_lines[0].time, Some(1.0));
    for input in [
        json!([]),
        json!({"lrc":{"lyric":null}}),
        json!({"lrc":{"offset":"1"}}),
        json!({"lrc":{"lyric":"{\"t\":0,\"c\":[]}\n[00:01]body"}}),
        json!({"lrc":{"lyric":"{\"t\":\"1000\",\"c\":[{\"tx\":\"credit\"}]}\n[00:01]body"}}),
    ] {
        assert!(matches!(parse_lyrics(&input), Err(Error::Response(_))));
    }
    assert!(format_timestamp(f64::NAN).is_err());
    assert!(
        render_lrc(&[LyricLine {
            time: Some(f64::INFINITY),
            lyric: "".into()
        }])
        .is_err()
    );
    let text = format!("[{},1]bad\n[0,1]ok", "9".repeat(400));
    assert_eq!(parse_yrc(&text).lyric_lines.len(), 1);
}

#[test]
fn timestamp_protection_is_idempotent_and_preserves_line_endings() {
    let text = " [00:01] \r\n[0,100]\r[00:03]words\n[00:04]\n";
    let protected = protect_timestamp_only_lines(text);
    assert_eq!(
        protected,
        " [00:01] \u{200b}\r\n[0,100]\u{200b}\r[00:03]words\n[00:04]\u{200b}\n"
    );
    assert_eq!(protect_timestamp_only_lines(&protected), protected);
    let parsed = parse_lrc(&protected);
    assert_eq!(parsed.lyric_lines.len(), 3);
    assert_eq!(parsed.lyric_lines[0].time, Some(1.0));
}

#[test]
fn word_times_preserve_unicode_and_invalid_word_bounds_fall_back_to_text() {
    let lines = cumulus_core::lyrics::parse_yrc_words(
        "[1000,2000](1000,500,0)你(1500,500,0)好\n[4000,500](4000,800,0)too long\n[6000,100]plain",
    );
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].words[1].text, "好");
    assert_eq!(lines[0].words[1].time, 1.5);
    assert_eq!(lines[0].words[1].duration, 0.5);
    assert!(lines[1].words.is_empty());
    assert!(lines[1].text.contains("too"));
    assert!(lines[2].words.is_empty());
    assert_eq!(lines[2].text, "plain ");
    assert!(cumulus_core::lyrics::parse_yrc_words("[999999999999999999999,10](0,0,0)x").is_empty());
    let mixed = parse_yrc_words("[1000,2000](1000,500,0)Hello(1500,500,0)world(2000,500,0)你好");
    assert_eq!(mixed[0].words[0].text, "Hello ");
    assert_eq!(
        mixed[0]
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<String>(),
        mixed[0].text
    );
    let prefix = parse_yrc_words("[1000,2000]prefix(1000,1000,0)word");
    assert!(prefix[0].words.is_empty());
    assert!(prefix[0].text.contains("prefix"));
}
