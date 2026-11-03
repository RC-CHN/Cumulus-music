//! Exact parameter calls made by cloudmusic EF9ED0 / EF8CA0.
//! Gain limits belong to the native setters; do not pre-clamp this call plan.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum EqualizationCall {
    EqGains { gains: [f32; 10] },
    EqOn { value: bool },
    BtGains { bass: f32, treble: f32 },
    BtOn { value: bool },
}

/// Short arrays are zero-padded; only the first ten elements are considered.
/// Nonnumeric elements become zero rather than retaining the previous entry.
/// Missing/mistyped eqs makes no gain call, but still sends the on/off call.
pub fn graphic_calls(section: &Value) -> Result<Vec<EqualizationCall>> {
    let section = super::object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on && let Some(values) = section.get("eqs").and_then(Value::as_array) {
        let mut gains = [0.0; 10];
        for (gain, value) in gains.iter_mut().zip(values) {
            *gain = super::number(Some(value))?.unwrap_or(0.0);
        }
        result.push(EqualizationCall::EqGains { gains });
    }
    result.push(EqualizationCall::EqOn { value: on });
    Ok(result)
}

/// The native JSON bridge passes only two gains. bassFreq/bassQ/trebleFreq/
/// trebleQ are not read here and must not be applied from the material. Missing
/// bass defaults to +2 dB, treble to 0 dB. Parameters precede the switch call.
pub fn bass_treble_calls(section: &Value) -> Result<Vec<EqualizationCall>> {
    let section = super::object(section)?;
    let on = section.get("on").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Vec::new();
    if on {
        result.push(EqualizationCall::BtGains {
            bass: super::number(section.get("bass"))?.unwrap_or(2.0),
            treble: super::number(section.get("treble"))?.unwrap_or(0.0),
        });
    }
    result.push(EqualizationCall::BtOn { value: on });
    Ok(result)
}
