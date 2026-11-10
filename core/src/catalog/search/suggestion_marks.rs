//! PC 3.1.42.205594 M/123.d, as consumed by M/87.b on M/623's suggestions.
const GLYPH_PAIRS: &[u8; 9404] = include_bytes!("suggestion_glyph_pairs.bin");
const MAX_SCALARS: usize = 256;
const MAX_MARKS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Part<'a> {
    pub text: &'a str,
    pub marked: bool,
}

pub(super) struct SuggestionMatcher {
    first: Vec<char>,
    second: Vec<char>,
}

impl SuggestionMatcher {
    pub(super) fn new(query: &str) -> Self {
        Self::from_normalized(&normalize(query))
    }

    fn from_normalized(normalized: &str) -> Self {
        // M/123 escapes literal syntax, then replaces only the first
        // ASCII space with an alternative. Keep both clauses literal.
        let pattern = normalized
            .trim_matches(js_whitespace)
            .split(' ')
            .filter(|piece| !piece.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let (left, right) = pattern.split_once(' ').unwrap_or((&pattern, ""));
        Self {
            first: left.chars().map(legacy_case).collect(),
            second: right.chars().map(legacy_case).collect(),
        }
    }

    pub(super) fn parts<'a>(&self, label: &'a str) -> Vec<Part<'a>> {
        let label = label.trim_matches(js_whitespace);
        // A width-converted query can produce an empty pattern. Skip the
        // original's repeated zero-length marks; preserve plain display.
        if self.first.is_empty() || label.chars().nth(MAX_SCALARS).is_some() {
            return vec![Part {
                text: label,
                marked: false,
            }];
        }

        let (mut offsets, units): (Vec<_>, Vec<_>) = label
            .char_indices()
            .map(|(offset, c)| (offset, legacy_case(c)))
            .unzip();
        offsets.push(label.len());
        let mut parts = Vec::new();
        let mut from = 0;
        let mut pos = 0;
        let mut marked = 0;
        while pos < units.len() && marked < MAX_MARKS {
            let suffix = &units[pos..];
            let width = if suffix.starts_with(&self.first) {
                self.first.len()
            } else if !self.second.is_empty() && suffix.starts_with(&self.second) {
                self.second.len()
            } else {
                pos += 1;
                continue;
            };
            let start = offsets[pos];
            if from < start {
                parts.push(Part {
                    text: &label[from..start],
                    marked: false,
                });
            }
            pos += width;
            from = offsets[pos];
            parts.push(Part {
                text: &label[start..from],
                marked: true,
            });
            marked += 1;
        }
        if from < label.len() {
            parts.push(Part {
                text: &label[from..],
                marked: false,
            });
        }
        parts
    }
}

fn normalize(query: &str) -> String {
    if query.chars().nth(MAX_SCALARS).is_some() {
        return String::new();
    }
    query
        .split(js_whitespace)
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .chars()
        .map(|c| simplified(width(c)))
        .collect()
}

// M/123.f searches the normalized query, then calls u with that already
// normalized query. On a miss, u receives no query and displays the first
// alias plainly. Preserve the original filtered-index behavior for "".
pub(super) fn first_alias<'a>(aliases: &[&'a str], query: &str) -> Vec<Part<'a>> {
    let normalized = normalize(query);
    let matcher = SuggestionMatcher::from_normalized(&normalized);
    let found = aliases
        .iter()
        .filter(|alias| !alias.is_empty())
        .position(|alias| matcher.parts(alias).iter().any(|part| part.marked));
    let parts = if let Some(index) = found {
        SuggestionMatcher::new(&normalized).parts(aliases[index])
    } else if let Some(alias) = aliases.first() {
        SuggestionMatcher::new("").parts(alias)
    } else {
        vec![]
    };
    parts
        .into_iter()
        .filter(|part| !part.text.is_empty())
        .collect()
}

fn js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | ' '
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

fn width(c: char) -> char {
    match c {
        '\u{FF00}'..='\u{FFFF}' => {
            char::from_u32(u32::from(c) - 0xFEE0).expect("valid width mapping")
        }
        _ => c,
    }
}

fn simplified(c: char) -> char {
    let Ok(unit) = u16::try_from(u32::from(c)) else {
        return c;
    };
    // Compact, sorted BMP pairs; duplicate sources retain the first index
    // from the M/123 pair strings. Each entry was checked in the original.
    let (mut lo, mut hi) = (0, GLYPH_PAIRS.len() / 4);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let at = mid * 4;
        let key = u16::from_le_bytes([GLYPH_PAIRS[at], GLYPH_PAIRS[at + 1]]);
        match unit.cmp(&key) {
            std::cmp::Ordering::Less => hi = mid,
            std::cmp::Ordering::Greater => lo = mid + 1,
            std::cmp::Ordering::Equal => {
                let target = u16::from_le_bytes([GLYPH_PAIRS[at + 2], GLYPH_PAIRS[at + 3]]);
                return char::from_u32(u32::from(target)).unwrap_or(c);
            }
        }
    }
    c
}

// Non-u RegExp Canonicalize uppercases a single UTF-16 unit. Surrogate pairs
// in valid UTF-8 compare exactly as pairs, with no supplementary case folding.
fn legacy_case(c: char) -> char {
    if c.len_utf16() != 1 {
        return c;
    }
    let mut upper = c.to_uppercase();
    let first = upper.next().expect("Unicode uppercase is nonempty");
    if upper.next().is_some() || first.len_utf16() != 1 || (!c.is_ascii() && first.is_ascii()) {
        c
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    const VECTORS: &str =
        include_str!("../../../tests/fixtures/search-suggestion-marks-official.json");
    const MANIFEST: &str =
        include_str!("../../../tests/fixtures/search-suggestion-marks-official.manifest.json");

    #[test]
    fn frozen_original_factory_spans_and_utf16_offsets() {
        let cases: Value = serde_json::from_str(VECTORS).unwrap();
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(
            manifest["table"]["sha256"],
            format!("{:x}", Sha256::digest(GLYPH_PAIRS))
        );
        assert_eq!(
            manifest["vectors"]["sha256"],
            format!("{:x}", Sha256::digest(VECTORS))
        );

        let vectors = cases["vectors"].as_array().unwrap();
        assert_eq!(manifest["vectors"]["validScalarCases"], vectors.len());
        let mut exact_count = 0;
        for case in vectors {
            let name = case["name"].as_str().unwrap();
            let label = case["label"].as_str().unwrap();
            let query = case["keyword"].as_str().unwrap();
            let parts = SuggestionMatcher::new(query).parts(label);
            match case["scope"].as_str().unwrap() {
                "exact" => {
                    exact_count += 1;
                    let actual = parts
                        .iter()
                        .map(|part| json!({"text":part.text,"match":part.marked}))
                        .collect::<Vec<_>>();
                    assert_eq!(
                        actual.as_slice(),
                        case["originalParts"].as_array().unwrap(),
                        "{name}"
                    );
                    let mut offset = 0;
                    let matches = parts
                        .iter()
                        .filter_map(|part| {
                            let start = offset;
                            offset += part.text.encode_utf16().count();
                            part.marked.then_some(json!([start, offset]))
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        matches.as_slice(),
                        case["originalMatchUtf16"].as_array().unwrap(),
                        "{name}"
                    );
                }
                "plain_for_empty_pattern" | "plain_for_input_bound" => {
                    assert_eq!(
                        parts,
                        [Part {
                            text: label,
                            marked: false
                        }],
                        "{name}"
                    );
                    assert!(
                        case["originalParts"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|part| part["match"] == true),
                        "{name}: original actually diverges"
                    );
                }
                _ => panic!("{name}: unknown fixture scope"),
            }
        }
        assert_eq!(exact_count, 30);
        assert_eq!(cases["unsupportedUtf16"].as_array().unwrap().len(), 3);
    }
}
