//! Bounded desktop lyric projection. The recovered JS specifies styles and
//! auxiliary-mode selection; native C++ line animation is reconstructed here.
use super::{DisplayType, LyricLine, WordLine};
use crate::{
    Error, Result,
    preferences::desktop_lyrics::{Appearance, Auxiliary},
};
use serde::Serialize;
mod prepared;
pub use prepared::Prepared;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub duration: f64,
    pub progress: f64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub index: usize,
    pub text: String,
    pub auxiliary: String,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub active: bool,
    pub progress: f64,
    pub words: Vec<Word>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub display_type: DisplayType,
    pub timed_words: bool,
    pub position: f64,
    pub offset: f64,
    /// Always at most two original lines; auxiliary text is paired, never a
    /// second copy of the full lyric document.
    pub lines: Vec<Line>,
}
/// Borrowed parsed input avoids cloning the complete document on every tick.
pub struct Source<'a> {
    pub lines: &'a [LyricLine],
    pub translation: &'a [LyricLine],
    pub romanization: &'a [LyricLine],
    pub word_lines: &'a [WordLine],
    pub word_translation: &'a [LyricLine],
    pub word_romanization: &'a [LyricLine],
    pub offset: f64,
    pub display_type: DisplayType,
}
fn progress(position: f64, start: f64, end: Option<f64>) -> f64 {
    if position < start {
        return 0.0;
    }
    match end {
        Some(end) if end > start => {
            if position >= end {
                1.0
            } else {
                let span = end - start;
                let fraction = if span.is_finite() {
                    (position - start) / span
                } else {
                    // Only scale an overflowing span: scaling tiny durations
                    // would otherwise round them down to zero.
                    (position * 0.5 - start * 0.5) / (end * 0.5 - start * 0.5)
                };
                fraction.clamp(0.0, 1.0)
            }
        }
        _ => 1.0,
    }
}
impl Source<'_> {
    pub fn frame(&self, position: f64, appearance: &Appearance) -> Result<Frame> {
        if !position.is_finite() || !self.offset.is_finite() {
            return Err(Error::InvalidInput("invalid desktop lyric clock"));
        }
        appearance.validate()?;
        let (words, order) = self.order();
        self.frame_order(position, appearance, words, &order, false)
    }
    fn order(&self) -> (bool, Vec<(usize, Option<f64>)>) {
        let words = !self.word_lines.is_empty();
        let mut order: Vec<(usize, Option<f64>)> = if words {
            self.word_lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| {
                    let time = l.time - self.offset;
                    time.is_finite().then_some((i, Some(time)))
                })
                .collect()
        } else {
            self.lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| l.time.is_none_or(f64::is_finite).then_some((i, l.time)))
                .collect()
        };
        // A plain-text lyric has no timing evidence; show its first lines with
        // no invented karaoke timing. Mixed untimed credits are not clock rows.
        let timed = order.iter().any(|(_, time)| time.is_some());
        if timed {
            order.retain(|(_, time)| time.is_some());
            order.sort_by(|(ia, a), (ib, b)| {
                if a == b {
                    // Signed zero denotes the same lyric time. total_cmp
                    // would put -0 before +0 and lose source-order precedence.
                    ia.cmp(ib)
                } else {
                    a.unwrap().total_cmp(&b.unwrap())
                }
            });
            // The recovered active_line rule chooses the later duplicate.
            order.dedup_by(|a, b| {
                if a.1 == b.1 {
                    b.0 = a.0;
                    true
                } else {
                    false
                }
            });
        }
        (words, order)
    }
    fn frame_order(
        &self,
        position: f64,
        appearance: &Appearance,
        words: bool,
        order: &[(usize, Option<f64>)],
        auxiliary_aligned: bool,
    ) -> Result<Frame> {
        if !position.is_finite() || !self.offset.is_finite() {
            return Err(Error::InvalidInput("invalid desktop lyric clock"));
        }
        appearance.validate()?;
        let timed = order.first().is_some_and(|(_, time)| time.is_some());
        let current = if timed {
            order
                .partition_point(|(_, time)| time.is_some_and(|v| v <= position))
                .saturating_sub(1)
        } else {
            0
        };
        let count = if appearance.single_line { 1 } else { 2 };
        let mut result = Vec::with_capacity(count);
        for cursor in current..(current + count).min(order.len()) {
            let (index, start) = order[cursor];
            let next = order.get(cursor + 1).and_then(|(_, time)| *time);
            let active = start.is_some_and(|v| v <= position) && cursor == current;
            let (text, auxiliary, end, fragments) = if words {
                let line = &self.word_lines[index];
                let rows = match appearance.auxiliary {
                    Auxiliary::None => &[][..],
                    Auxiliary::Translate if !self.word_translation.is_empty() => {
                        self.word_translation
                    }
                    Auxiliary::Translate => self.translation,
                    Auxiliary::Roman if !self.word_romanization.is_empty() => {
                        self.word_romanization
                    }
                    Auxiliary::Roman => self.romanization,
                };
                let auxiliary = (if auxiliary_aligned {
                    rows.get(index)
                } else {
                    rows.iter().find(|r| r.time == Some(line.time))
                })
                .map(|r| r.lyric.clone())
                .unwrap_or_default();
                let end = (line.time + line.duration - self.offset)
                    .is_finite()
                    .then_some(line.time + line.duration - self.offset);
                let fragments = line
                    .words
                    .iter()
                    .filter_map(|word| {
                        let begin = word.time - self.offset;
                        if !begin.is_finite()
                            || !word.duration.is_finite()
                            || word.duration < 0.0
                            || !(begin + word.duration).is_finite()
                        {
                            return None;
                        }
                        Some(Word {
                            text: word.text.clone(),
                            start: begin,
                            duration: word.duration,
                            progress: progress(position, begin, Some(begin + word.duration)),
                        })
                    })
                    .collect();
                (line.text.clone(), auxiliary, end, fragments)
            } else {
                let rows = match appearance.auxiliary {
                    Auxiliary::None => &[][..],
                    Auxiliary::Translate => self.translation,
                    Auxiliary::Roman => self.romanization,
                };
                (
                    self.lines[index].lyric.clone(),
                    rows.get(index).map(|l| l.lyric.clone()).unwrap_or_default(),
                    next,
                    vec![],
                )
            };
            result.push(Line {
                index,
                text,
                auxiliary,
                start,
                end,
                active,
                progress: start.map(|v| progress(position, v, end)).unwrap_or(0.0),
                words: fragments,
            });
        }
        Ok(Frame {
            display_type: self.display_type,
            timed_words: words,
            position,
            offset: self.offset,
            lines: result,
        })
    }
}
