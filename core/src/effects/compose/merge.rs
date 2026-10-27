use super::numeric;
use crate::{Error, Result, protocol::wire::serialization::js_truthy};
use serde_json::{Map, Value, json};

fn target<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>> {
    let object = value
        .as_object_mut()
        .ok_or(Error::InvalidInput("effect merge target must be an object"))?;
    let entry = object.entry(key.to_owned()).or_insert_with(|| json!({}));
    if !js_truthy(entry) {
        *entry = json!({});
    }
    entry
        .as_object_mut()
        .ok_or(Error::InvalidInput("effect section must be an object"))
}
fn assign(target: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), value.clone());
    } else {
        target.shift_remove(key);
    }
}
fn or_zero(value: Option<&Value>) -> Value {
    value
        .filter(|value| js_truthy(value))
        .cloned()
        .unwrap_or(json!(0))
}
fn cmp(target: &mut Map<String, Value>, on: bool) -> Result<&mut Map<String, Value>> {
    let value = target.entry("cmp".to_owned()).or_insert(Value::Null);
    if !js_truthy(value) {
        *value = json!({"attack":0,"on":on,"pregain":0,"release":0,"range":0,"ratio":1,"postgain":0,"threshold":0});
    }
    value
        .as_object_mut()
        .ok_or(Error::InvalidInput("compressor section must be an object"))
}
pub(super) fn merge(
    base: &Value,
    song: &Value,
    diy: &Map<String, Value>,
    stereo: bool,
) -> Result<Option<Value>> {
    let is_song = js_truthy(song);
    let mut value = if is_song {
        song.clone()
    } else if js_truthy(base) {
        let base = base
            .as_object()
            .ok_or(Error::InvalidInput("base must be an object"))?;
        if base.len() > 1 {
            return Ok(None);
        }
        base.values().next().cloned().unwrap_or(json!({}))
    } else {
        json!({})
    };
    let key = if is_song { "aev2" } else { "effect" };
    let active = |name: &str| diy.get(name).is_some_and(js_truthy);
    let bass = active("en_bass");
    let treble = active("en_treble");
    let count = usize::from(bass)
        + usize::from(treble)
        + usize::from(active("en_vocal"))
        + usize::from(active("en_vibe") || active("en_live"));
    if bass || treble {
        let bt = if bass && treble {
            let low = diy["en_bass"]
                .get("bt")
                .filter(|v| !v.is_null())
                .ok_or(Error::InvalidInput("missing bass BT section"))?;
            let high = diy["en_treble"]
                .get("bt")
                .filter(|v| !v.is_null())
                .ok_or(Error::InvalidInput("missing treble BT section"))?;
            Some(
                json!({"on":true,"bass":or_zero(low.get("bass")),"bassFreq":or_zero(low.get("bassFreq")),"bassQ":or_zero(low.get("bassQ")),
                "treble":or_zero(high.get("treble")),"trebleFreq":or_zero(high.get("trebleFreq")),"trebleQ":or_zero(high.get("trebleQ"))}),
            )
        } else {
            diy[if bass { "en_bass" } else { "en_treble" }]
                .get("bt")
                .cloned()
        };
        assign(target(&mut value, key)?, "bt", bt.as_ref());
    }
    if active("en_vocal") {
        assign(target(&mut value, key)?, "peq", diy["en_vocal"].get("peq"));
    }
    if active("en_vibe") {
        let section = target(&mut value, key)?;
        if is_song
            && let Some(rvb) = section.get_mut("rvb").and_then(Value::as_object_mut)
            && rvb.get("on") == Some(&Value::Bool(true))
        {
            rvb.insert("on".to_owned(), json!(false));
        }
        assign(section, "se", diy["en_vibe"].get("se"));
        if let Some(add) = diy["en_vibe"].get("cmp").filter(|value| js_truthy(value)) {
            let cmp = cmp(section, false)?;
            let sum = numeric::sum(&or_zero(cmp.get("pregain")), &or_zero(add.get("pregain")))?;
            cmp.insert("pregain".to_owned(), sum);
        }
    }
    if active("en_live") {
        assign(target(&mut value, key)?, "rvb", diy["en_live"].get("rvb"));
    }
    if count > 0 {
        let cmp = cmp(target(&mut value, key)?, true)?;
        let sum = numeric::sum(
            &or_zero(cmp.get("pregain")),
            &json!([0.0, -0.5, -1.2, -2.1, -3.0][count]),
        )?;
        cmp.insert("pregain".to_owned(), sum);
    }
    if stereo
        && let Some(eqs) = value
            .get_mut(key)
            .and_then(|v| v.get_mut("eq"))
            .and_then(|v| v.get_mut("eqs"))
            .and_then(Value::as_array_mut)
    {
        for eq in eqs {
            *eq = numeric::finite(0.8 * numeric::number(eq)?)?;
        }
    }
    if let Some(cmp) = value
        .get_mut(key)
        .and_then(|v| v.get_mut("cmp"))
        .and_then(Value::as_object_mut)
        && let Some(pregain) = cmp.get("pregain").and_then(Value::as_f64)
    {
        cmp.insert("on".to_owned(), json!(pregain != 0.0));
    }
    // Undefined song.aev2 serializes as an absent member, not explicit null.
    let mut normalized = Map::new();
    if is_song {
        assign(&mut normalized, "aev2", value.get("aev2"));
    } else {
        normalized.insert(
            "aev2".to_owned(),
            value
                .get("effect")
                .filter(|v| js_truthy(v))
                .cloned()
                .unwrap_or(json!({})),
        );
    }
    Ok(Some(Value::Object(normalized)))
}
