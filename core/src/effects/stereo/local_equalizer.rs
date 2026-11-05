//! Typed old-schema local Equalizer material, independent of selected effects
//! or credentials. This deliberately excludes active RVB and remote materials.
use crate::{Error, Result};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalEqualizerMaterialPlan {
    pub(super) geq: Option<[f32; 10]>,
    pub(super) tone: [i32; 2],
    pub(super) stereo: [i32; 2],
}

impl LocalEqualizerMaterialPlan {
    /// Parse the old M3091 local equalizer contract. An off RVB object may
    /// still contain inactive controls, as the official None preset does.
    /// This parses settings only; it does not authorize a source or effect.
    pub fn from_equalizer_json(value: &Value) -> Result<Self> {
        let invalid = || Error::InvalidInput("unsupported local equalizer material");
        let root = value
            .as_object()
            .filter(|x| x.len() == 4)
            .ok_or_else(invalid)?;
        let eq = section(root, "eq", 2).ok_or_else(invalid)?;
        let bt = section(root, "bt", 3).ok_or_else(invalid)?;
        let se = section(root, "se", 5).ok_or_else(invalid)?;
        if root
            .get("rvb")
            .and_then(Value::as_object)
            .and_then(|rvb| boolean(rvb, "on"))
            != Some(false)
            || boolean(se, "sshaper") != Some(true)
            || se.get("ambience").and_then(Value::as_i64) != Some(1)
        {
            return Err(invalid());
        }
        let eqs = eq
            .get("eqs")
            .and_then(Value::as_array)
            .filter(|x| x.len() == 10)
            .ok_or_else(invalid)?;
        let mut gains = [0.0; 10];
        let mut eq_on = false;
        for (gain, number) in gains.iter_mut().zip(eqs) {
            let value = number
                .as_f64()
                .filter(|x| (-12.0..=12.0).contains(x))
                .ok_or_else(invalid)?;
            eq_on |= value != 0.0;
            *gain = value as f32;
        }
        let tone = [
            bounded_integer(bt, "bass", -10, 10).ok_or_else(invalid)?,
            bounded_integer(bt, "treble", -10, 10).ok_or_else(invalid)?,
        ];
        let stereo = [
            bounded_integer(se, "presence", -10, 10).ok_or_else(invalid)?,
            bounded_integer(se, "stereoizer", 0, 10).ok_or_else(invalid)?,
        ];
        if boolean(eq, "on") != Some(eq_on)
            || boolean(bt, "on") != Some(tone != [0, 0])
            || boolean(se, "on") != Some(stereo != [0, 0])
        {
            return Err(invalid());
        }
        Ok(Self {
            geq: eq_on.then_some(gains),
            tone,
            stereo,
        })
    }
}

fn section<'a>(
    root: &'a Map<String, Value>,
    key: &str,
    len: usize,
) -> Option<&'a Map<String, Value>> {
    root.get(key)?.as_object().filter(|x| x.len() == len)
}
fn boolean(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key)?.as_bool()
}
fn bounded_integer(map: &Map<String, Value>, key: &str, min: i32, max: i32) -> Option<i32> {
    let value = map.get(key)?.as_i64()?;
    (i64::from(min)..=i64::from(max))
        .contains(&value)
        .then_some(value as i32)
}
