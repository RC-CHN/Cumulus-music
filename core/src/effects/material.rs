//! Official component parameter parsing. Enumerating a PEQ fragment does not
//! imply that the full native material selector would activate that fragment.
mod controls;
mod equalization;
mod routing;
use super::dsp::{Coefficients, ParametricBand, ParametricEq, ParametricKind, RotationStage};
use crate::{Error, Result};
pub use controls::{
    CompressorParameters, ControlCall, DelayParameters, LimiterParameters, alc_calls,
    compressor_calls, delay_calls, limiter_calls, speaker_spatial_calls,
};
pub use equalization::{EqualizationCall, bass_treble_calls, graphic_calls};
pub use routing::{Component, ComponentSection, MaterialRouting, MaterialSchema, route_sections};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "lowercase")]
pub enum PeqCall {
    Gain {
        value: f32,
    },
    Band {
        band: u32,
        #[serde(rename = "type")]
        kind: u32,
        gain: f32,
        freq: f32,
        q: f32,
        on: bool,
    },
    On {
        value: bool,
    },
}

fn object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or(Error::InvalidInput("effect section must be an object"))
}

fn number(value: Option<&Value>) -> Result<Option<f32>> {
    let Some(number) = value.and_then(Value::as_f64) else {
        return Ok(None);
    };
    let converted = number as f32;
    if !converted.is_finite() {
        return Err(Error::InvalidInput("non-finite effect parameter"));
    }
    Ok(Some(converted))
}

fn integer(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|number| i32::try_from(number).ok())
}

/// EFC380: missing/mistyped fields retain prior locals across array elements.
/// Non-objects count toward the first nine positions but emit no band call.
pub fn peq_calls(section: &Value) -> Result<Vec<PeqCall>> {
    let section = object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut calls = Vec::new();
    if on {
        if let Some(value) = number(section.get("gain"))? {
            calls.push(PeqCall::Gain { value });
        }
        if let Some(filters) = section.get("f").and_then(Value::as_array) {
            let (mut band, mut kind) = (0i32, 1i32);
            let (mut freq, mut gain, mut q, mut enabled) = (80.0, 0.0, 0.0, true);
            for item in filters.iter().take(9).filter_map(Value::as_object) {
                if let Some(value) = integer(item.get("band")) {
                    band = value;
                }
                if let Some(value) = integer(item.get("type")) {
                    kind = value;
                }
                if let Some(value) = number(item.get("freq"))? {
                    freq = value;
                }
                if let Some(value) = number(item.get("gain"))? {
                    gain = value;
                }
                if let Some(value) = number(item.get("q"))? {
                    q = value;
                }
                if let Some(value) = item.get("on").and_then(Value::as_bool) {
                    enabled = value;
                }
                calls.push(PeqCall::Band {
                    band: band as u32,
                    kind: kind as u32,
                    gain,
                    freq,
                    q,
                    on: enabled,
                });
            }
        }
    }
    calls.push(PeqCall::On { value: on });
    Ok(calls)
}

/// Validate the complete plan before changing any live parameter. Out-of-range
/// band numbers are ignored as the native outer setter does; unsafe filter
/// types and non-finite/impossible coefficients reject the entire plan.
pub fn apply_peq(stage: &mut ParametricEq, calls: &[PeqCall]) -> Result<()> {
    for call in calls {
        match *call {
            PeqCall::Gain { value } if !value.is_finite() => {
                return Err(Error::InvalidInput("non-finite effect gain"));
            }
            PeqCall::Band {
                band,
                kind,
                gain,
                freq,
                q,
                ..
            } if band < 9 => {
                if !gain.is_finite() {
                    return Err(Error::InvalidInput("non-finite effect gain"));
                }
                Coefficients::parametric(
                    ParametricKind::try_from(kind)?,
                    stage.format().rate(),
                    freq,
                    gain.clamp(-24.0, 24.0),
                    q,
                )?;
            }
            _ => {}
        }
    }
    for call in calls {
        match *call {
            PeqCall::Gain { value } => stage.set_gain(value)?,
            PeqCall::Band {
                band,
                kind,
                gain,
                freq,
                q,
                on,
            } if band < 9 => stage.set_band(
                band as usize,
                ParametricBand {
                    kind: ParametricKind::try_from(kind)?,
                    gain_db: gain,
                    frequency: freq,
                    q,
                    enabled: on,
                },
            )?,
            PeqCall::On { value } => stage.set_bypass(!value)?,
            _ => {}
        }
    }
    Ok(())
}

pub fn peq_sections(material: &Value) -> Result<Vec<(&'static str, &Value)>> {
    let material = object(material)?;
    let mut result = Vec::new();
    if let Some(section) = material.get("peq").filter(|value| value.is_object()) {
        result.push(("peq", section));
    }
    if let Some(section) = material
        .get("aev2")
        .and_then(|value| value.get("peq"))
        .filter(|value| value.is_object())
    {
        result.push(("aev2.peq", section));
    }
    Ok(result)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "call", rename_all = "lowercase")]
pub enum RotationCall {
    Velocity { value: f32 },
    On { value: bool },
}

/// The material's velocity field means seconds per rotation. Values below one
/// are ignored by the native setter; malformed values reject the whole plan.
pub fn apply_rotation(stage: &mut RotationStage, calls: &[RotationCall]) -> Result<()> {
    if calls
        .iter()
        .any(|call| matches!(call, RotationCall::Velocity { value } if !value.is_finite()))
    {
        return Err(Error::InvalidInput("non-finite rotation period"));
    }
    for call in calls {
        match *call {
            RotationCall::Velocity { value } => {
                stage.set_period(value)?;
            }
            RotationCall::On { value } => stage.set_bypass(!value)?,
        }
    }
    Ok(())
}

pub fn rotation_calls(section: &Value) -> Result<Vec<RotationCall>> {
    let section = object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on {
        result.push(RotationCall::Velocity {
            value: number(section.get("velocity"))?.unwrap_or(20.0),
        });
    }
    result.push(RotationCall::On { value: on });
    Ok(result)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "call", rename_all = "lowercase")]
pub enum StereoCall {
    Stereo {
        presence: f32,
        stereoizer: f32,
        sshaper: bool,
        ambience: i32,
    },
    On {
        value: bool,
    },
}

pub fn stereo_calls(section: &Value) -> Result<Vec<StereoCall>> {
    let section = object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on {
        result.push(StereoCall::Stereo {
            presence: number(section.get("presence"))?.unwrap_or(0.0),
            stereoizer: number(section.get("stereoizer"))?.unwrap_or(0.0),
            sshaper: section
                .get("sshaper")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ambience: integer(section.get("ambience")).unwrap_or(0),
        });
    }
    result.push(StereoCall::On { value: on });
    Ok(result)
}

/// EFB760's recovered spatial branch: an object aev2 wins even when empty.
/// No fallback to legacy se/rotate occurs for missing modern components.
pub fn spatial_sections(material: &Value) -> Result<Vec<(&'static str, &Value)>> {
    let material = object(material)?;
    let modern = material.get("aev2").and_then(Value::as_object);
    let selected = modern.unwrap_or(material);
    Ok([("se", "aev2.se"), ("rotate", "aev2.rotate")]
        .into_iter()
        .filter_map(|(legacy, current)| {
            selected
                .get(legacy)
                .filter(|value| value.is_object())
                .map(|value| (if modern.is_some() { current } else { legacy }, value))
        })
        .collect())
}
