//! Ten-band equalizer settings and a native, versioned LADSPA host boundary.
//! This is the recovered GEQ component, not the complete proprietary effect graph.
pub mod abi;
mod host;
mod preferences;
use crate::{Error, Result};
pub use host::EqualizerHost;
pub use preferences::{
    EqualizerPreferences, EqualizerPreset, EqualizerSnapshot, EqualizerSpatialStatus,
    EqualizerStatus,
};
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
    #[serde(skip_serializing_if = "is_zero")]
    presence: i8,
    #[serde(skip_serializing_if = "is_zero")]
    stereoizer: i8,
    // Missing values are old records whose single switch gated both stages.
    #[serde(skip_serializing_if = "Option::is_none")]
    tone_enabled: Option<bool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    master_bypassed: bool,
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
    #[serde(default)]
    presence: i8,
    #[serde(default)]
    stereoizer: i8,
    #[serde(default)]
    tone_enabled: Option<bool>,
    #[serde(default)]
    master_bypassed: bool,
}
impl TryFrom<RawSettings> for EqualizerSettings {
    type Error = Error;
    fn try_from(raw: RawSettings) -> Result<Self> {
        let mut settings = Self::new(raw.enabled, raw.gains)?
            .with_tone(raw.bass_db.into(), raw.treble_db.into())?
            .with_stereo_controls(raw.presence.into(), raw.stereoizer.into())?;
        settings.tone_enabled = raw.tone_enabled;
        settings.master_bypassed = raw.master_bypassed;
        Ok(settings)
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
            presence: 0,
            stereoizer: 0,
            tone_enabled: None,
            master_bypassed: false,
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
    /// Original local M3091 enhanced controls. This does not authorize a
    /// decoder, pick an arbitrary material bank, or enable the stock filter.
    pub fn with_stereo_controls(mut self, presence: i32, stereoizer: i32) -> Result<Self> {
        if !(-10..=10).contains(&presence) || !(0..=10).contains(&stereoizer) {
            return Err(Error::InvalidInput("invalid local stereo effect controls"));
        }
        self.presence = presence as i8;
        self.stereoizer = stereoizer as i8;
        Ok(self)
    }

    pub fn stereo_controls(self) -> (i32, i32) {
        (self.presence.into(), self.stereoizer.into())
    }

    pub(crate) fn stereo_active(self) -> bool {
        !self.master_bypassed && self.stereo_controls() != (0, 0)
    }

    pub(crate) fn local_graph_requested(self) -> bool {
        self.effective().enabled() || self.stereo_active()
    }

    pub fn geq_active(self) -> bool {
        !self.master_bypassed && self.enabled && self.gains.iter().any(|g| *g != 0.0)
    }
    pub fn tone_enabled(self) -> bool {
        self.tone_enabled.unwrap_or(self.enabled)
    }
    /// A selected local EQ can request activation even with a flat curve.
    /// Keep its saved master switch separate from a transient source bypass.
    pub(crate) fn requests_activation(self) -> bool {
        !self.master_bypassed && (self.enabled || self.tone_enabled())
    }
    pub(crate) fn master_bypassed(self) -> bool {
        self.master_bypassed
    }
    pub(crate) fn with_master_bypassed(mut self, bypassed: bool) -> Self {
        self.master_bypassed = bypassed;
        self
    }
    pub fn with_tone_enabled(mut self, enabled: bool) -> Self {
        self.tone_enabled = Some(enabled);
        self
    }
    pub(crate) fn same_parameters(self, other: Self) -> bool {
        self.master_bypassed == other.master_bypassed
            && self.enabled == other.enabled
            && self.gains == other.gains
            && self.tone() == other.tone()
            && self.stereo_controls() == other.stereo_controls()
            && self.tone_enabled() == other.tone_enabled()
    }
    pub fn tone_active(self) -> bool {
        !self.master_bypassed && self.tone_enabled() && self.tone() != (0, 0)
    }
    /// M/3091 switches GEQ and bass/treble separately. The v2 DSP ABI
    /// still takes one overall bit; pass zero gains for each bypassed stage.
    /// Stored preferences keep their original (possibly bypassed) gains.
    pub fn effective(self) -> Self {
        let geq = self.geq_active();
        let tone = self.tone_active();
        Self {
            enabled: geq || tone,
            gains: if geq { self.gains } else { [0.0; 10] },
            bass_db: if tone { self.bass_db } else { 0 },
            treble_db: if tone { self.treble_db } else { 0 },
            presence: 0,
            stereoizer: 0,
            tone_enabled: None,
            master_bypassed: false,
        }
    }
}
