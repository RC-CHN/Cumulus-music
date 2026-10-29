use super::*;
use std::collections::HashMap;
fn aligned(words: &[WordLine], primary: &[LyricLine], fallback: &[LyricLine]) -> Vec<LyricLine> {
    let rows = if primary.is_empty() {
        fallback
    } else {
        primary
    };
    let key = |time: f64| if time == 0.0 { 0 } else { time.to_bits() };
    let mut index = HashMap::new();
    for row in rows {
        if let Some(time) = row.time.filter(|v| v.is_finite()) {
            index.entry(key(time)).or_insert(row.lyric.as_str());
        }
    }
    words
        .iter()
        .map(|word| LyricLine {
            time: Some(word.time),
            lyric: index
                .get(&key(word.time))
                .copied()
                .unwrap_or_default()
                .into(),
        })
        .collect()
}
/// Immutable document prepared once per lyric revision. Clock ticks only find
/// a position in the sorted index and project one or two lines.
pub struct Prepared {
    lines: Vec<LyricLine>,
    translation: Vec<LyricLine>,
    romanization: Vec<LyricLine>,
    word_lines: Vec<WordLine>,
    word_translation: Vec<LyricLine>,
    word_romanization: Vec<LyricLine>,
    offset: f64,
    display_type: DisplayType,
    words: bool,
    order: Vec<(usize, Option<f64>)>,
}
impl Source<'_> {
    pub fn prepare(&self) -> Prepared {
        let (words, order) = self.order();
        Prepared {
            lines: self.lines.to_vec(),
            translation: self.translation.to_vec(),
            romanization: self.romanization.to_vec(),
            word_lines: self.word_lines.to_vec(),
            word_translation: aligned(self.word_lines, self.word_translation, self.translation),
            word_romanization: aligned(self.word_lines, self.word_romanization, self.romanization),
            offset: self.offset,
            display_type: self.display_type,
            words,
            order,
        }
    }
}
impl Prepared {
    pub fn frame(&self, position: f64, appearance: &Appearance) -> Result<Frame> {
        Source {
            lines: &self.lines,
            translation: &self.translation,
            romanization: &self.romanization,
            word_lines: &self.word_lines,
            word_translation: &self.word_translation,
            word_romanization: &self.word_romanization,
            offset: self.offset,
            display_type: self.display_type,
        }
        .frame_order(position, appearance, self.words, &self.order, true)
    }
}
