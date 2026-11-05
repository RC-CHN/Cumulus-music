use super::PaletteColor;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

/// A missing gradient means the exact `commonTheme.colorBlack8` token. It is
/// deliberately not replaced with a guessed light/dark RGB value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneFmBackground {
    ThemeFallback,
    Gradient {
        first: PaletteColor,
        middle: PaletteColor,
    },
}

impl SceneFmBackground {
    /// Malformed-but-truthy hex can preserve JS NaN in the generated CSS. A
    /// renderer must not interpret that as permission to fetch another image.
    pub fn css(self) -> Option<String> {
        match self {
            Self::ThemeFallback => None,
            Self::Gradient { first, middle } => Some(format!(
                "linear-gradient(245deg, {} 0%, {} 48%, {} 100%)",
                first.css(),
                middle.css(),
                first.css()
            )),
        }
    }
}

/// M54:52–99. JS Math.max/min propagate NaN; Rust f64::max/min do not.
pub fn rgb_to_hsv(color: PaletteColor) -> Hsv {
    let normalized =
        |value: Option<i16>| value.map_or(f64::NAN, |v| bound(f64::from(v), 255.0, false));
    let [r, g, b] = [
        normalized(color.r),
        normalized(color.g),
        normalized(color.b),
    ];
    let (max, min) = if [r, g, b].iter().any(|v| v.is_nan()) {
        (f64::NAN, f64::NAN)
    } else {
        (r.max(g).max(b), r.min(g).min(b))
    };
    let delta = max - min;
    let s = if max == 0.0 { 0.0 } else { delta / max };
    let mut h = 0.0;
    if max != min {
        if max == r {
            h = (g - b) / delta + if g < b { 6.0 } else { 0.0 };
        } else if max == g {
            h = (b - r) / delta + 2.0;
        } else if max == b {
            h = (r - g) / delta + 4.0;
        }
        h /= 6.0;
    }
    Hsv {
        h: 360.0 * h,
        s,
        v: max,
    }
}

fn bound(value: f64, limit: f64, percent: bool) -> f64 {
    if value.is_nan() {
        return f64::NAN;
    }
    let mut value = value.clamp(0.0, limit);
    if percent {
        value = (value * limit).trunc() / 100.0;
    }
    if (value - limit).abs() < 1e-6 {
        1.0
    } else {
        (value % limit) / limit
    }
}

fn unit(value: f64) -> f64 {
    if value <= 1.0 {
        bound(100.0 * value, 100.0, true)
    } else {
        bound(value, 100.0, false)
    }
}

/// M54:100–138, including percentage truncation and positive JS Math.round.
pub fn hsv_to_rgb(hsv: Hsv) -> PaletteColor {
    let h = 6.0 * bound(hsv.h, 360.0, false);
    let s = unit(hsv.s);
    let v = unit(hsv.v);
    if h.is_nan() {
        return PaletteColor {
            r: None,
            g: None,
            b: None,
        };
    }
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let sector = (i as usize) % 6;
    let channel = |value: f64| (!value.is_nan()).then(|| (255.0 * value).round() as i16);
    PaletteColor {
        r: channel([v, q, p, p, t, v][sector]),
        g: channel([t, v, v, q, p, p][sector]),
        b: channel([p, p, t, v, v, q][sector]),
    }
}

/// SceneFm.tsx:291–303. In particular, valid pure green/blue/black use fallback.
pub fn background(color: Option<PaletteColor>) -> SceneFmBackground {
    let Some(color) = color.filter(|color| color.r.is_some_and(|r| r > 0)) else {
        return SceneFmBackground::ThemeFallback;
    };
    let mut hsv = rgb_to_hsv(color);
    if hsv.s > 0.04 {
        hsv.s = 0.6;
    }
    hsv.v = 0.7;
    SceneFmBackground::Gradient {
        first: hsv_to_rgb(hsv),
        middle: hsv_to_rgb(Hsv {
            h: (hsv.h + 350.0) % 360.0,
            ..hsv
        }),
    }
}
