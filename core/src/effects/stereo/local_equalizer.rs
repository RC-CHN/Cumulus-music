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
    pub(crate) fn from_saved(settings: crate::effects::equalizer::EqualizerSettings) -> Self {
        let (bass, treble) = settings.tone();
        let (presence, stereoizer) = settings.stereo_controls();
        Self {
            geq: settings.geq_active().then(|| *settings.gains()),
            tone: if settings.tone_active() {
                [bass, treble]
            } else {
                [0, 0]
            },
            stereo: if settings.stereo_active() {
                [presence, stereoizer]
            } else {
                [0, 0]
            },
        }
    }

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

#[cfg(test)]
mod saved_controls_tests {
    use super::*;
    use crate::effects::equalizer::EqualizerSettings;
    use serde_json::json;

    #[test]
    fn saved_local_spatial_controls_match_the_official_m3091_material_without_stock_enable() {
        let zeros = [0.0; 10];
        let old = json!({"enabled":false,"gains":zeros});
        let legacy: EqualizerSettings = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(serde_json::to_value(legacy).unwrap(), old);
        assert!(!legacy.local_graph_requested());

        let controls = legacy.with_stereo_controls(-10, 7).unwrap();
        let original = LocalEqualizerMaterialPlan::from_equalizer_json(&json!({
            "eq":{"on":false,"eqs":zeros},
            "bt":{"on":false,"bass":0,"treble":0},
            "rvb":{"on":false},
            "se":{"on":true,"presence":-10,"stereoizer":7,"sshaper":true,"ambience":1}
        }))
        .unwrap();
        assert_eq!(LocalEqualizerMaterialPlan::from_saved(controls), original);
        assert!(controls.local_graph_requested());
        assert_eq!(controls.effective(), legacy);

        let mut persisted = serde_json::to_value(controls).unwrap();
        persisted["master_bypassed"] = json!(true);
        let bypassed: EqualizerSettings = serde_json::from_value(persisted.clone()).unwrap();
        assert_eq!(bypassed.stereo_controls(), (-10, 7));
        assert!(!bypassed.local_graph_requested());
        assert_eq!(
            LocalEqualizerMaterialPlan::from_saved(bypassed),
            LocalEqualizerMaterialPlan::from_saved(legacy)
        );
        for (name, value) in [
            ("presence", json!(11)),
            ("stereoizer", json!(-1)),
            ("presence", json!(2.5)),
            ("stereoizer", json!(11)),
        ] {
            let mut invalid = persisted.clone();
            invalid[name] = value;
            assert!(serde_json::from_value::<EqualizerSettings>(invalid).is_err());
        }
    }
}
