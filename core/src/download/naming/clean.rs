use crate::protocol::wire::serialization::is_js_whitespace;

pub(super) fn clean(text: &str) -> String {
    let mapped: String = text
        .chars()
        .filter_map(|c| match c {
            '\\' => Some('＼'),
            '/' | '*' | '?' | '<' | '>' | ':' | '"' | '|' => None,
            _ => Some(c),
        })
        .collect();
    mapped
        .trim_matches(|c: char| c == ',' || c == '.' || is_js_whitespace(c) || c <= '\u{1f}')
        .chars()
        .filter(|c| *c > '\u{1f}')
        .collect()
}
pub(super) fn units(text: &str) -> usize {
    text.encode_utf16().count()
}
pub(super) fn prefix(text: &str, budget: usize) -> &str {
    let mut used = 0;
    for (offset, c) in text.char_indices() {
        used += c.len_utf16();
        if used > budget {
            return &text[..offset];
        }
    }
    text
}
