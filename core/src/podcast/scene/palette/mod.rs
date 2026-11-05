//! PC 3.1.42 M54/M258 Scene FM cover colors, independent of image I/O.
//!
//! Header truthiness, malformed hex channels and the original quantizer's
//! filter/storage behavior are intentional. RGBA inputs are already sampled
//! pixels: this module neither decodes images nor reproduces browser resizing,
//! color management or alpha premultiplication.
mod color;
mod header;
mod quantize;

pub use color::{Hsv, SceneFmBackground, background, hsv_to_rgb, rgb_to_hsv};
pub use header::{HeaderDecision, Palette, parse_header};
pub use quantize::{MAX_RGBA_PIXELS, SampleError, sample_rgba};

/// A channel is `None` exactly when JS `parseInt` produced NaN. This is distinct
/// from a missing header color. Quantized pixels always have three channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PaletteColor {
    pub r: Option<i16>,
    pub g: Option<i16>,
    pub b: Option<i16>,
}

impl PaletteColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: Some(r as i16),
            g: Some(g as i16),
            b: Some(b as i16),
        }
    }

    pub fn css(self) -> String {
        fn channel(value: Option<i16>) -> String {
            value.map_or_else(|| "NaN".into(), |v| v.to_string())
        }
        format!(
            "rgb({}, {}, {})",
            channel(self.r),
            channel(self.g),
            channel(self.b)
        )
    }
}
