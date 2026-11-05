use super::PaletteColor;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    pub dominant: Option<PaletteColor>,
    pub top: Option<PaletteColor>,
    pub bottom: Option<PaletteColor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderDecision {
    /// Missing/empty header, or a known field throws while being parsed.
    Sample,
    /// Any successful object is truthy, including no recognized fields. Do not
    /// request a second sample just because `dominant` is absent or malformed.
    Parsed(Palette),
}

/// M54:196–215,244–260. This accepts response metadata only, never a URL.
pub fn parse_header(header: Option<&str>) -> HeaderDecision {
    let Some(header) = header.filter(|v| !v.is_empty()) else {
        return HeaderDecision::Sample;
    };
    let mut palette = Palette::default();
    for field in header.split('|') {
        let mut pieces = field.split(':');
        let key = pieces.next().unwrap_or_default();
        let target = match key {
            "M" => Some(&mut palette.dominant),
            "T" => Some(&mut palette.top),
            "B" => Some(&mut palette.bottom),
            _ => None,
        };
        if target.is_none() && !object_prototype_key(key) {
            continue;
        }
        let Some(value) = pieces.next() else {
            return HeaderDecision::Sample;
        };
        let Some(color) = hex_color(value.split(';').next().unwrap_or_default()) else {
            return HeaderDecision::Sample;
        };
        if let Some(target) = target {
            *target = Some(color);
        }
    }
    HeaderDecision::Parsed(palette)
}

fn object_prototype_key(key: &str) -> bool {
    // The original M/T/B lookup uses a normal JS object, not an own-property
    // check. Inherited truthy properties still parse their value and can throw;
    // successfully added non-color fields do not belong in the typed palette.
    matches!(
        key,
        "constructor"
            | "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__proto__"
            | "toLocaleString"
    )
}

fn hex_color(value: &str) -> Option<PaletteColor> {
    // JS substrings and .length use UTF-16 code units. Invalid/non-ASCII units
    // remain parseInt NaN rather than causing UTF-8 slicing or a new fallback.
    let units = value.encode_utf16().collect::<Vec<_>>();
    let (r, g, b) = match units.as_slice() {
        [r, g, b] => ([*r, *r], [*g, *g], [*b, *b]),
        [r1, r2, g1, g2, b1, b2] => ([*r1, *r2], [*g1, *g2], [*b1, *b2]),
        _ => return None,
    };
    Some(PaletteColor {
        r: parse_hex_pair(r),
        g: parse_hex_pair(g),
        b: parse_hex_pair(b),
    })
}

fn parse_hex_pair(units: [u16; 2]) -> Option<i16> {
    // parseInt(...,16) skips JS whitespace, accepts a sign, strips 0x, and
    // stops at the first non-hex character. Only two units are available here.
    let mut chars = units.into_iter().peekable();
    while chars.peek().is_some_and(|c| js_whitespace(*c)) {
        chars.next();
    }
    let negative = match chars.peek() {
        Some(0x2b) => {
            chars.next();
            false
        }
        Some(0x2d) => {
            chars.next();
            true
        }
        _ => false,
    };
    let remaining = chars.collect::<Vec<_>>();
    if remaining.as_slice() == [0x30, 0x78] || remaining.as_slice() == [0x30, 0x58] {
        return None;
    }
    let mut number = None;
    for unit in remaining {
        let digit = match unit {
            0x30..=0x39 => unit - 0x30,
            0x41..=0x46 => unit - 0x41 + 10,
            0x61..=0x66 => unit - 0x61 + 10,
            _ => break,
        } as i16;
        number = Some(number.unwrap_or(0) * 16 + digit);
    }
    number.map(|v| if negative { -v } else { v })
}

fn js_whitespace(c: u16) -> bool {
    matches!(
        c,
        0x0009..=0x000d
            | 0x0020
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028..=0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    )
}
