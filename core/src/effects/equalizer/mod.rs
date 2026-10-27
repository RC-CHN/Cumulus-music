//! Ten-band equalizer settings and a native, versioned LADSPA host boundary.
//! This is the recovered GEQ component, not the complete proprietary effect graph.
pub mod abi;
mod host;
mod preferences;
use crate::{Error, Result};
pub use host::EqualizerHost;
pub use preferences::{EqualizerPreferences, EqualizerSnapshot, EqualizerStatus};
use serde::{Deserialize, Serialize};

/// Official M/3091 sliders: ten dB values, each in [-12, 12]. Labels are rounded
/// for presentation; DSP frequencies/endpoint shelves remain in the recovered C.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawSettings")]
pub struct EqualizerSettings {
    enabled: bool,
    gains: [f32; 10],
}

#[derive(Deserialize)]
struct RawSettings {
    enabled: bool,
    gains: [f32; 10],
}
impl TryFrom<RawSettings> for EqualizerSettings {
    type Error = Error;
    fn try_from(raw: RawSettings) -> Result<Self> {
        Self::new(raw.enabled, raw.gains)
    }
}
impl EqualizerSettings {
    pub fn new(enabled: bool, gains: [f32; 10]) -> Result<Self> {
        if gains
            .iter()
            .any(|g| !g.is_finite() || !(-12.0..=12.0).contains(g))
        {
            return Err(Error::InvalidInput(
                "equalizer gains must be finite and within +/-12 dB",
            ));
        }
        Ok(Self { enabled, gains })
    }
    pub fn enabled(self) -> bool {
        self.enabled
    }
    pub fn gains(&self) -> &[f32; 10] {
        &self.gains
    }
    /// Official M/3091 emits eq.on=false for an all-zero curve, independently
    /// of the page's overall enabled switch.
    pub fn effective(self) -> Self {
        Self {
            enabled: self.enabled && self.gains.iter().any(|g| *g != 0.0),
            ..self
        }
    }
}
