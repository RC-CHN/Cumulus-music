use super::{numeric, ordered_keys};
use crate::{Result, protocol::wire::serialization::js_truthy};
use serde_json::{Map, Value};

fn below(value: Option<&Value>, threshold: f64) -> Result<bool> {
    Ok(value
        .map(numeric::number)
        .transpose()?
        .is_some_and(|value| value < threshold))
}
fn above(value: Option<&Value>, threshold: f64) -> Result<bool> {
    Ok(value
        .map(numeric::number)
        .transpose()?
        .is_some_and(|value| value > threshold))
}
fn bass(source: &Value, song: bool) -> Result<bool> {
    if !js_truthy(source) {
        return Ok(false);
    }
    if !song {
        return Ok(true);
    }
    if below(
        source.pointer("/feature/vocal_drum_bass_other_ratio/1"),
        0.05,
    )? {
        return Ok(true);
    }
    if let (Some(eqs), Some(low)) = (
        source.pointer("/aev2/eq/eqs").and_then(Value::as_array),
        source.pointer("/feature/low_mid_high_ratio/0"),
    ) && eqs.len() >= 3
    {
        return Ok(numeric::average([&eqs[0], &eqs[1], &eqs[2]])?
            + 6.0 * (1.0 - numeric::number(low)?)
            > 6.0);
    }
    Ok(false)
}
fn treble(source: &Value, song: bool) -> Result<bool> {
    if !js_truthy(source) {
        return Ok(false);
    }
    if song
        && (above(source.pointer("/feature/vocal_main_freq"), 600.0)?
            || above(source.pointer("/feature/audio_spectral_centroid"), 4000.0)?)
    {
        return Ok(true);
    }
    // Base closing rule intentionally reads effect at the wrapper depth.
    if let Some(eqs) = source
        .pointer(if song {
            "/aev2/eq/eqs"
        } else {
            "/effect/eq/eqs"
        })
        .and_then(Value::as_array)
        && eqs.len() >= 3
    {
        let n = eqs.len();
        return Ok(numeric::average([&eqs[n - 1], &eqs[n - 2], &eqs[n - 3]])? > 5.0);
    }
    Ok(false)
}
pub(super) fn filter(
    base: &Value,
    song: &Value,
    diy: &Map<String, Value>,
    stereo: bool,
) -> Result<Map<String, Value>> {
    let mut result = Map::new();
    if stereo {
        return Ok(result);
    }
    let is_song = js_truthy(song);
    let source = if is_song { song } else { base };
    for key in ordered_keys(diy)? {
        let closed = match key.as_str() {
            "en_bass" => bass(source, is_song)?,
            "en_treble" => treble(source, is_song)?,
            "en_vocal" => {
                js_truthy(source)
                    && (!is_song
                        || below(
                            source.pointer("/feature/vocal_drum_bass_other_ratio/0"),
                            0.2,
                        )?)
            }
            "en_vibe" | "en_live" => continue,
            _ => false,
        };
        if !closed {
            result.insert(key.clone(), diy[&key].clone());
        }
    }
    match (diy.contains_key("en_vibe"), diy.contains_key("en_live")) {
        (true, true) => {
            let key = if is_song && source.pointer("/aev2/rvb/on") != Some(&Value::Bool(true)) {
                "en_live"
            } else {
                "en_vibe"
            };
            result.insert(key.to_owned(), diy[key].clone());
        }
        (true, false) if js_truthy(&diy["en_vibe"]) => {
            result.insert("en_vibe".to_owned(), diy["en_vibe"].clone());
        }
        (false, true) if js_truthy(&diy["en_live"]) => {
            result.insert("en_live".to_owned(), diy["en_live"].clone());
        }
        _ => {}
    }
    Ok(result)
}
