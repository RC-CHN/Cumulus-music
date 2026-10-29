//! CUE is a view over one album file. Clip every lyric channel using the
//! original row identity, and translate word clocks into clip-relative time.
use super::*;
use crate::lyrics::{LyricLine, WordLine};
fn indices(times: &[Option<f64>], start: f64, length: Option<f64>) -> Vec<usize> {
    let previous = times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.filter(|v| *v < start).map(|v| (i, v)))
        .max_by(|(ia, a), (ib, b)| a.total_cmp(b).then(ia.cmp(ib)))
        .map(|(i, _)| i);
    let mut selected: Vec<_> = times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            (t.is_none_or(|t| t >= start && length.is_none_or(|end| t - start < end))).then_some(i)
        })
        .collect();
    if !selected.iter().any(|i| times[*i] == Some(start))
        && let Some(previous) = previous
    {
        selected.insert(0, previous);
    }
    selected
}
fn auxiliary(rows: &[LyricLine], selected: &[usize], start: f64) -> Vec<LyricLine> {
    if rows.is_empty() {
        return vec![];
    }
    selected
        .iter()
        .map(|i| {
            let mut row = rows.get(*i).cloned().unwrap_or(LyricLine {
                time: None,
                lyric: String::new(),
            });
            row.time = row.time.map(|t| t - start);
            row
        })
        .collect()
}
fn word_auxiliary(
    rows: &[LyricLine],
    old: &[WordLine],
    selected: &[usize],
    new: &[WordLine],
) -> Vec<LyricLine> {
    if rows.is_empty() {
        return vec![];
    }
    selected
        .iter()
        .zip(new)
        .map(|(i, line)| LyricLine {
            time: Some(line.time),
            lyric: rows
                .iter()
                .find(|r| r.time == Some(old[*i].time))
                .map(|r| r.lyric.clone())
                .unwrap_or_default(),
        })
        .collect()
}
pub(super) fn apply(view: &mut LyricsView, start: f64, length: Option<f64>) {
    let selected = indices(
        &view
            .word_lines
            .iter()
            .map(|l| Some(l.time - view.offset))
            .collect::<Vec<_>>(),
        start,
        length,
    );
    let words: Vec<_> = selected
        .iter()
        .map(|i| {
            let mut line = view.word_lines[*i].clone();
            let end = line.time + line.duration - start;
            line.time = (line.time - start).max(view.offset);
            line.duration = (end - line.time).max(0.0);
            for word in &mut line.words {
                word.time -= start;
            }
            line
        })
        .collect();
    view.word_translation = word_auxiliary(
        if view.word_translation.is_empty() {
            &view.translation
        } else {
            &view.word_translation
        },
        &view.word_lines,
        &selected,
        &words,
    );
    view.word_romanization = word_auxiliary(
        if view.word_romanization.is_empty() {
            &view.romanization
        } else {
            &view.word_romanization
        },
        &view.word_lines,
        &selected,
        &words,
    );
    view.word_lines = words;
    let selected = indices(
        &view.lines.iter().map(|l| l.time).collect::<Vec<_>>(),
        start,
        length,
    );
    view.translation = auxiliary(&view.translation, &selected, start);
    view.romanization = auxiliary(&view.romanization, &selected, start);
    view.lines = selected
        .iter()
        .map(|i| {
            let mut line = view.lines[*i].clone();
            line.time = line.time.map(|t| (t - start).max(0.0));
            line
        })
        .collect();
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{DisplayType, LyricPreferences, TimedWord, YrcInfo};
    fn fixture(offset: f64) -> LyricsView {
        let row = |time, lyric: &str| LyricLine {
            time: Some(time),
            lyric: lyric.into(),
        };
        LyricsView {
            epoch: "1".into(),
            play_id: "album".into(),
            lines: vec![
                row(10.0 - offset, "first"),
                row(29.0 - offset, "crossing"),
                row(35.0 - offset, "next"),
                row(60.0 - offset, "outside"),
            ],
            translation: vec![
                row(10.0, "第一首"),
                row(29.0, "跨界"),
                row(35.0, "下一行"),
                row(60.0, "界外"),
            ],
            romanization: vec![
                row(10.0, "a"),
                row(29.0, "b"),
                row(35.0, "c"),
                row(60.0, "d"),
            ],
            word_lines: [10.0, 29.0, 35.0, 60.0]
                .into_iter()
                .map(|time| WordLine {
                    time,
                    duration: 4.0,
                    text: format!("at {time}"),
                    words: vec![
                        TimedWord {
                            time,
                            duration: 2.0,
                            text: "before".into(),
                        },
                        TimedWord {
                            time: time + 2.0,
                            duration: 2.0,
                            text: "after".into(),
                        },
                    ],
                })
                .collect(),
            word_translation: vec![
                row(10.0, "一"),
                row(29.0, "跨"),
                row(35.0, "次"),
                row(60.0, "外"),
            ],
            word_romanization: vec![
                row(10.0, "one"),
                row(29.0, "cross"),
                row(35.0, "next"),
                row(60.0, "out"),
            ],
            words: YrcInfo::default(),
            offset,
            scrollable: true,
            display_type: DisplayType::Default,
            preferences: LyricPreferences::default(),
            network_failed: false,
            from_cache: false,
        }
    }
    #[test]
    fn cue_preserves_auxiliary_row_identity_and_word_clocks_across_a_boundary() {
        let mut value = fixture(0.0);
        apply(&mut value, 30.0, Some(30.0));
        assert_eq!(
            value
                .lines
                .iter()
                .map(|l| l.lyric.as_str())
                .collect::<Vec<_>>(),
            ["crossing", "next"]
        );
        assert_eq!(
            value.lines.iter().map(|l| l.time).collect::<Vec<_>>(),
            [Some(0.0), Some(5.0)]
        );
        assert_eq!(
            value
                .translation
                .iter()
                .map(|l| l.lyric.as_str())
                .collect::<Vec<_>>(),
            ["跨界", "下一行"]
        );
        assert_eq!(value.romanization[0].lyric, "b");
        assert_eq!(value.word_lines[0].time, 0.0);
        assert_eq!(value.word_lines[0].duration, 3.0);
        assert_eq!(value.word_lines[0].words[0].time, -1.0);
        assert_eq!(value.word_lines[0].words[1].time, 1.0);
        assert_eq!(value.word_translation[0].lyric, "跨");
        assert_eq!(value.word_translation[0].time, Some(0.0));
        assert_eq!(value.word_romanization[1].lyric, "next");
        assert_eq!(value.word_romanization[1].time, Some(5.0));
    }
    #[test]
    fn cue_word_auxiliary_can_fall_back_to_lrc_without_losing_the_crossing_line() {
        let mut value = fixture(-1.0);
        value.word_translation.clear();
        value.word_romanization.clear();
        apply(&mut value, 30.0, Some(30.0));
        assert_eq!(value.word_lines[0].time - value.offset, 0.0);
        assert_eq!(
            value.word_translation[0].time,
            Some(value.word_lines[0].time)
        );
        assert_eq!(value.word_translation[0].lyric, "跨界");
        assert_eq!(value.word_romanization[0].lyric, "b");
    }
    #[test]
    fn cue_offset_is_applied_once_and_an_exact_start_supersedes_previous_text() {
        let mut value = fixture(0.5);
        apply(&mut value, 30.0, Some(29.0));
        assert_eq!(value.lines.len(), 2);
        assert_eq!(value.lines[1].time, Some(4.5));
        assert_eq!(value.word_lines[0].time - value.offset, 0.0);
        assert_eq!(value.word_lines[0].words[1].time - value.offset, 0.5);
        assert_eq!(
            value.word_translation[0].time,
            Some(value.word_lines[0].time)
        );
        let mut exact = fixture(0.0);
        apply(&mut exact, 35.0, None);
        assert_eq!(exact.lines[0].lyric, "next");
        assert_eq!(exact.word_lines[0].text, "at 35");
        assert_eq!(exact.word_lines[0].time, 0.0);
    }
}
