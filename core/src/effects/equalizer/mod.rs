//! Ten-band equalizer settings and a native, versioned LADSPA host boundary.
//! This is the recovered GEQ component, not the complete proprietary effect graph.
pub mod abi;
mod host;
mod preferences;
use crate::{Error, Result};
pub use host::EqualizerHost;
pub use preferences::{EqualizerPreferences, EqualizerPreset, EqualizerSnapshot, EqualizerStatus};
use serde::{Deserialize, Serialize};

/// Official M/3091 sliders: ten dB values, each in [-12, 12]. Labels are rounded
/// for presentation; DSP frequencies/endpoint shelves remain in the recovered C.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawSettings")]
pub struct EqualizerSettings {
    enabled: bool,
    gains: [f32; 10],
    #[serde(skip_serializing_if = "is_zero")]
    bass_db: i8,
    #[serde(skip_serializing_if = "is_zero")]
    treble_db: i8,
}

fn is_zero(value: &i8) -> bool {
    *value == 0
}
#[derive(Deserialize)]
struct RawSettings {
    enabled: bool,
    gains: [f32; 10],
    #[serde(default)]
    bass_db: i8,
    #[serde(default)]
    treble_db: i8,
}
impl TryFrom<RawSettings> for EqualizerSettings {
    type Error = Error;
    fn try_from(raw: RawSettings) -> Result<Self> {
        Self::new(raw.enabled, raw.gains)?.with_tone(raw.bass_db.into(), raw.treble_db.into())
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
        Ok(Self {
            enabled,
            gains,
            bass_db: 0,
            treble_db: 0,
        })
    }
    pub fn enabled(self) -> bool {
        self.enabled
    }
    pub fn gains(&self) -> &[f32; 10] {
        &self.gains
    }
    /// Recovered enhanced-effect sliders: integer dB in [-10, 10].
    pub fn with_tone(mut self, bass_db: i32, treble_db: i32) -> Result<Self> {
        if !(-10..=10).contains(&bass_db) || !(-10..=10).contains(&treble_db) {
            return Err(Error::InvalidInput("tone gains must be within +/-10 dB"));
        }
        self.bass_db = bass_db as i8;
        self.treble_db = treble_db as i8;
        Ok(self)
    }
    pub fn tone(self) -> (i32, i32) {
        (self.bass_db.into(), self.treble_db.into())
    }
    pub fn geq_active(self) -> bool {
        self.enabled && self.gains.iter().any(|g| *g != 0.0)
    }
    pub fn tone_active(self) -> bool {
        self.enabled && self.tone() != (0, 0)
    }
    /// Official M/3091 emits eq.on=false for an all-zero curve, independently
    /// of the page's overall enabled switch.
    pub fn effective(self) -> Self {
        Self {
            enabled: self.geq_active() || self.tone_active(),
            ..self
        }
    }
}
