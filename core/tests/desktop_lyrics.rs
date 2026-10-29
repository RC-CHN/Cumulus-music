use cumulus_core::{
    lyrics::{DisplayType, LyricLine, TimedWord, WordLine, desktop::Source},
    preferences::desktop_lyrics::{Appearance, Auxiliary},
};
fn line(time: Option<f64>, text: &str) -> LyricLine {
    LyricLine {
        time,
        lyric: text.into(),
    }
}
fn source(lines: &[LyricLine]) -> Source<'_> {
    Source {
        lines,
        translation: &[],
        romanization: &[],
        word_lines: &[],
        word_translation: &[],
        word_romanization: &[],
        offset: 0.0,
        display_type: DisplayType::Default,
    }
}
#[test]
fn zero_intro_gap_duplicate_unsorted_and_two_line_bounds_are_deterministic() {
    let lines = [
        line(Some(8.0), "eight"),
        line(None, "credit"),
        line(Some(0.0), "zero"),
        line(Some(4.0), "old four"),
        line(Some(4.0), "new four"),
    ];
    let aux = [
        line(Some(8.0), "八"),
        line(None, ""),
        line(Some(0.0), "零"),
        line(Some(4.0), "旧"),
        line(Some(4.0), "新"),
    ];
    let input = Source {
        translation: &aux,
        ..source(&lines)
    };
    let prepared = input.prepare();
    let style = Appearance {
        single_line: false,
        ..Default::default()
    };
    let before = prepared.frame(-0.2, &style).unwrap();
    assert_eq!(before.lines[0].text, "zero");
    assert!(!before.lines[0].active);
    assert_eq!(before.lines[0].progress, 0.0);
    let now = prepared.frame(5.0, &style).unwrap();
    assert_eq!(now.lines.len(), 2);
    assert_eq!(now.lines[0].text, "new four");
    assert_eq!(now.lines[0].auxiliary, "新");
    assert_eq!(now.lines[0].progress, 0.25);
    assert_eq!(now.lines[1].text, "eight");
    assert!(!now.lines[1].active);
    let end = prepared.frame(99.0, &style).unwrap();
    assert_eq!(end.lines.len(), 1);
    assert_eq!(end.lines[0].progress, 1.0);
    for at in [-1.0, 0.0, 2.0, 4.0, 4.5, 8.0, 9.0] {
        assert_eq!(
            serde_json::to_value(input.frame(at, &style).unwrap()).unwrap(),
            serde_json::to_value(prepared.frame(at, &style).unwrap()).unwrap()
        );
    }
}
#[test]
fn word_clock_offset_and_auxiliary_are_exact_and_zero_duration_is_finite() {
    let words = [WordLine {
        time: 1.0,
        duration: 2.0,
        text: "two words".into(),
        words: vec![
            TimedWord {
                time: 1.0,
                duration: 1.0,
                text: "two ".into(),
            },
            TimedWord {
                time: 2.0,
                duration: 0.0,
                text: "words".into(),
            },
        ],
    }];
    let translated = [line(Some(1.0), "两个词")];
    let roman = [line(Some(1.0), "liang ge ci")];
    let input = Source {
        word_lines: &words,
        word_translation: &translated,
        word_romanization: &roman,
        offset: 0.5,
        ..source(&[])
    };
    let style = Appearance::default();
    let now = input.prepare().frame(1.0, &style).unwrap();
    assert!(now.timed_words);
    assert_eq!(now.lines[0].start, Some(0.5));
    assert_eq!(now.lines[0].auxiliary, "两个词");
    assert_eq!(now.lines[0].words[0].progress, 0.5);
    assert_eq!(now.lines[0].words[1].progress, 0.0);
    let roman = input
        .frame(
            1.5,
            &Appearance {
                auxiliary: Auxiliary::Roman,
                ..style.clone()
            },
        )
        .unwrap();
    assert_eq!(roman.lines[0].auxiliary, "liang ge ci");
    assert_eq!(roman.lines[0].words[1].progress, 1.0);
    assert!(
        input
            .frame(
                1.0,
                &Appearance {
                    auxiliary: Auxiliary::None,
                    ..style.clone()
                }
            )
            .unwrap()
            .lines[0]
            .auxiliary
            .is_empty()
    );
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(input.frame(time, &style).is_err());
    }
}
#[test]
fn plain_text_empty_and_pure_music_do_not_invent_word_or_line_timing() {
    let lines = [
        line(None, "first"),
        line(None, "second"),
        line(None, "third"),
    ];
    let style = Appearance {
        single_line: false,
        ..Default::default()
    };
    let frame = source(&lines).frame(100.0, &style).unwrap();
    assert_eq!(frame.lines.len(), 2);
    assert!(
        frame
            .lines
            .iter()
            .all(|l| !l.active && l.start.is_none() && l.progress == 0.0)
    );
    let pure = Source {
        display_type: DisplayType::Pure,
        ..source(&[])
    }
    .frame(0.0, &style)
    .unwrap();
    assert!(pure.lines.is_empty());
    assert_eq!(pure.display_type, DisplayType::Pure);
    assert!(source(&[]).frame(0.0, &style).unwrap().lines.is_empty());
}

#[test]
fn prepared_words_keep_first_exact_auxiliary_match_and_finite_large_clock_progress() {
    let words = [
        WordLine {
            time: 0.0,
            duration: 1.0,
            text: "first".into(),
            words: vec![],
        },
        WordLine {
            time: -0.0,
            duration: 1.0,
            text: "second".into(),
            words: vec![],
        },
    ];
    let aux = [
        line(Some(-0.0), "first translation"),
        line(Some(0.0), "later translation"),
    ];
    let input = Source {
        word_lines: &words,
        word_translation: &aux,
        ..source(&[])
    };
    let style = Appearance::default();
    let raw = input.frame(0.0, &style).unwrap();
    let prepared = input.prepare().frame(0.0, &style).unwrap();
    assert_eq!(raw.lines[0].text, "second");
    assert_eq!(raw.lines[0].auxiliary, "first translation");
    assert_eq!(
        serde_json::to_value(raw).unwrap(),
        serde_json::to_value(prepared).unwrap()
    );
    let extremes = [
        line(Some(-f64::MAX), "earlier"),
        line(Some(f64::MAX), "later"),
    ];
    assert_eq!(
        source(&extremes).frame(0.0, &style).unwrap().lines[0].progress,
        0.5
    );
    let tiny = f64::from_bits(1);
    let shortest = [line(Some(0.0), "start"), line(Some(tiny), "end")];
    assert_eq!(
        source(&shortest).frame(0.0, &style).unwrap().lines[0].progress,
        0.0
    );
    let short = [line(Some(tiny), "start"), line(Some(tiny * 3.0), "end")];
    assert_eq!(
        source(&short).frame(tiny * 2.0, &style).unwrap().lines[0].progress,
        0.5
    );
}
