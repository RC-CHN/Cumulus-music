//! D00510/CFDC10 remove U+0020 after conversion; CEE8E0 checks the result.
//! The caller supplies the CRT locale operations and keeps the original units.

const PLACEHOLDERS: [&str; 5] = ["unknow", "oem", "o.e.m", "invalid", "diy"];

pub(super) fn board_serial(
    wide: &[u16],
    is_digit: impl Fn(u16) -> bool,
    lowercase: impl FnOnce(&mut [u16]),
) -> Vec<u16> {
    let mut cleaned: Vec<_> = wide
        .iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .filter(|unit| *unit != 0x20)
        .take(79)
        .collect();
    if cleaned.len() < 6 || cleaned.iter().copied().all(is_digit) {
        return Vec::new();
    }
    let mut folded = cleaned.clone();
    // The CRT gets a separate writable, NUL-terminated copy.
    folded.push(0);
    lowercase(&mut folded);
    let folded = &folded[..folded
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(folded.len())];
    if PLACEHOLDERS.iter().any(|word| {
        let word: Vec<_> = word.encode_utf16().collect();
        folded.windows(word.len()).any(|window| window == word)
    }) {
        cleaned.clear();
    }
    cleaned
}
