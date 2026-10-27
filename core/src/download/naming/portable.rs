use super::official;
use crate::Result;

pub(super) fn adapt(parts: &mut [String], suffix: &str, budget: usize) -> Result<bool> {
    let before = parts.to_vec();
    let last = parts.len() - 1;
    for (i, part) in parts.iter_mut().enumerate() {
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if device {
            part.insert(0, '_');
        }
        let limit = 255 - if i == last { suffix.len() + 1 } else { 0 };
        if part.len() > limit {
            let mut end = limit;
            while !part.is_char_boundary(end) {
                end -= 1;
            }
            part.truncate(end);
        }
        *part = part.trim_end_matches([' ', '.']).into();
    }
    official::fit(parts, budget)?;
    Ok(parts != before)
}
