//! Bounded plain-text search lyrics. Previewing three lines belongs to the UI;
//! never discard the remainder before the user can expand or copy it.
use serde_json::Value;

const MAX_BYTES: usize = 64 * 1024;
const MAX_LINES: usize = 512;

pub(super) fn project(value: &Value) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut bytes = 0;
    let values: Box<dyn Iterator<Item = &str> + '_> = if let Some(s) = value.as_str() {
        Box::new(std::iter::once(s))
    } else {
        Box::new(
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
    };
    for source in values {
        for line in source.split('\n') {
            if lines.len() == MAX_LINES {
                return (lines, true);
            }
            let separator = usize::from(!lines.is_empty());
            if bytes + separator > MAX_BYTES {
                return (lines, true);
            }
            let remaining = MAX_BYTES - bytes - separator;
            let end = line.floor_char_boundary(remaining.min(line.len()));
            // Existing matchedLyric compatibility; all other markup stays plain text.
            let text = super::results::clean(line[..end].trim_end_matches('\r'));
            bytes += text.len() + separator;
            lines.push(text);
            if end < line.len() {
                return (lines, true);
            }
        }
    }
    if lines.iter().all(|line| line.trim().is_empty()) {
        lines.clear();
    }
    (lines, false)
}
