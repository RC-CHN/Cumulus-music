//! Official native control calls; these are parameter plans, not DSP kernels.
//! cloudmusic EF9080/EFA280 use defaults, while EF99E0/EF8AC0/EFF1B0 can
//! deliberately skip the entire component when required fields are mistyped.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompressorParameters {
    pub threshold: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub range: f32,
    pub pregain: f32,
    pub postgain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimiterParameters {
    pub threshold: f32,
    pub attack: f32,
    pub release: f32,
    pub outputgain: f32,
    #[serde(rename = "infiniteRatio")]
    pub infinite_ratio: bool,
    pub ratio: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DelayParameters {
    pub delay: f32,
    pub feedback: f32,
    pub dry: f32,
    pub wet: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum ControlCall {
    CmpParameters { parameters: CompressorParameters },
    CmpOn { value: bool },
    LimiterParameters { parameters: LimiterParameters },
    LimiterOn { value: bool },
    DelayParameters { parameters: DelayParameters },
    DelayOn { value: bool },
    AlcOn { value: bool },
    SpeakerSpatialOn { value: bool },
}

pub fn compressor_calls(section: &Value) -> Result<Vec<ControlCall>> {
    let section = super::object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on {
        result.push(ControlCall::CmpParameters {
            parameters: CompressorParameters {
                threshold: super::number(section.get("threshold"))?.unwrap_or(-18.0),
                ratio: super::number(section.get("ratio"))?.unwrap_or(4.0),
                attack: super::number(section.get("attack"))?.unwrap_or(0.05),
                release: super::number(section.get("release"))?.unwrap_or(0.1),
                range: super::number(section.get("range"))?.unwrap_or(-5.0),
                pregain: super::number(section.get("pregain"))?.unwrap_or(0.0),
                postgain: super::number(section.get("postgain"))?.unwrap_or(0.0),
            },
        });
    }
    result.push(ControlCall::CmpOn { value: on });
    Ok(result)
}

/// Explicit `limiter.on=true` uses a -18 dB / 50 ms default. The engine's
/// unconditional material-begin limiter has different defaults; do not use
/// this function to initialize that node or assume missing means disabled.
pub fn limiter_calls(section: &Value) -> Result<Vec<ControlCall>> {
    let section = super::object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on {
        result.push(ControlCall::LimiterParameters {
            parameters: LimiterParameters {
                threshold: super::number(section.get("threshold"))?.unwrap_or(-18.0),
                attack: super::number(section.get("attack"))?.unwrap_or(0.05),
                release: super::number(section.get("release"))?.unwrap_or(0.1),
                outputgain: super::number(section.get("outputgain"))?.unwrap_or(0.0),
                infinite_ratio: section
                    .get("infiniteRatio")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                ratio: super::number(section.get("ratio"))?.unwrap_or(4.0),
            },
        });
    }
    result.push(ControlCall::LimiterOn { value: on });
    Ok(result)
}

/// The native bridge requires a boolean switch. When enabled, all four
/// numeric fields are required. A skipped object makes no parameter OR switch
/// call; it must not be treated as an explicit off request.
pub fn delay_calls(section: &Value) -> Result<Vec<ControlCall>> {
    let section = super::object(section)?;
    let Some(on) = section.get("on").and_then(Value::as_bool) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    if on {
        let fields = ["delay", "feedback", "dry", "wet"];
        if fields
            .iter()
            .any(|key| section.get(*key).and_then(Value::as_f64).is_none())
        {
            return Ok(result);
        }
        let mut values = [0.0; 4];
        for (value, key) in values.iter_mut().zip(fields) {
            *value = super::number(section.get(key))?.expect("numeric fields checked");
        }
        result.push(ControlCall::DelayParameters {
            parameters: DelayParameters {
                delay: values[0],
                feedback: values[1],
                dry: values[2],
                wet: values[3],
            },
        });
    }
    result.push(ControlCall::DelayOn { value: on });
    Ok(result)
}

pub fn alc_calls(section: &Value) -> Result<Vec<ControlCall>> {
    Ok(super::object(section)?
        .get("on")
        .and_then(Value::as_bool)
        .map(|value| ControlCall::AlcOn { value })
        .into_iter()
        .collect())
}

pub fn speaker_spatial_calls(section: &Value) -> Result<Vec<ControlCall>> {
    Ok(super::object(section)?
        .get("on")
        .and_then(Value::as_bool)
        .map(|value| ControlCall::SpeakerSpatialOn { value })
        .into_iter()
        .collect())
}
