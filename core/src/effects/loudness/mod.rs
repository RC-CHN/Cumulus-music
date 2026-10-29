//! Official M81.g conversion and cloudmusic's signed integer / 10000 bridge.
//! Missing, nonnumeric and zero adjustments disable the business-level switch.
use crate::{Error, Result};
use serde_json::Value;

pub mod abi;
mod host;
mod preferences;
pub use host::LoudnessHost;
pub use preferences::LoudnessPreferences;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoudnessStatus {
    #[default]
    Off,
    NoTrack,
    NoGain,
    InvalidGain,
    Pending,
    Configured,
    Processing,
    Unavailable,
    Failed,
}
#[derive(Clone, Debug, Default)]
pub struct LoudnessSnapshot {
    pub preferences: LoudnessPreferences,
    pub status: LoudnessStatus,
    pub damaged: bool,
    pub gain: Option<TrackGain>,
    pub invalid_gain: bool,
    pub has_track: bool,
}

/// Native adapter configuration. Business selection separately disables zero
/// or absent volumeDelta; this low-level boundary also supports neutral gain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoudnessSettings {
    enabled: bool,
    gain_db: f32,
}
impl LoudnessSettings {
    pub fn new(enabled: bool, gain_db: f32) -> Result<Self> {
        let mut probe = super::dsp::LoudnessStage::new(super::dsp::Format::new(48000.0, 1, 1)?);
        probe.set_parameters(super::dsp::LoudnessParameters { gain_db, peak: 0.0 })?;
        Ok(Self { enabled, gain_db })
    }
    pub fn enabled(self) -> bool {
        self.enabled
    }
    pub fn gain_db(self) -> f32 {
        self.gain_db
    }
}

/// The JSON-safe subset of M81.g. Exponential notation matters: parseInt(1e-8)
/// is 1 in JavaScript, so truncating the float would not preserve its behavior.
pub fn fixed_gain(value: &Value) -> Result<i32> {
    let Some(number) = value.as_f64() else {
        return Ok(0);
    };
    let scaled = number * 10_000.0;
    if !scaled.is_finite() {
        return Err(Error::InvalidInput("non-finite loudness adjustment"));
    }
    let mut buffer = ryu_js::Buffer::new();
    let text = buffer.format(scaled);
    let count = text
        .bytes()
        .enumerate()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == b'-'))
        .count();
    text[..count]
        .parse()
        .map_err(|_| Error::InvalidInput("loudness adjustment exceeds native integer range"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackGain {
    fixed: i32,
}

impl TrackGain {
    pub fn from_volume_delta(value: &Value) -> Result<Option<Self>> {
        let fixed = fixed_gain(value)?;
        Ok((fixed != 0).then_some(Self { fixed }))
    }
    pub fn fixed(self) -> i32 {
        self.fixed
    }
    pub fn decibels(self) -> f32 {
        (f64::from(self.fixed) / 10_000.0) as f32
    }
    pub fn parameters(self) -> super::dsp::LoudnessParameters {
        super::dsp::LoudnessParameters {
            gain_db: self.decibels(),
            // The official JS bridge sends {gain} only. cloudmusic reads the
            // omitted peak as integer zero, then divides both values by 10000.
            peak: 0.0,
        }
    }
}
