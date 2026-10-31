//! M/23.Z online predicate with the unobserved no-copyright MV experiment off.
//! This is candidate selection, never a substitute for the source authorization.
use crate::{Error, Result, protocol::wire::serialization::js_truthy};
use serde_json::Value;

fn number(value: &Value) -> Result<f64> {
    let n = match value {
        Value::Null => 0.0,
        Value::Bool(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(v) => v
            .as_f64()
            .ok_or(Error::Response("invalid playback number"))?,
        Value::String(v) => {
            let v = v.trim();
            if v.is_empty() {
                0.0
            } else if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16)
                    .map_err(|_| Error::Response("invalid playback number"))? as f64
            } else {
                v.parse()
                    .map_err(|_| Error::Response("invalid playback number"))?
            }
        }
        _ => return Err(Error::Response("invalid playback number")),
    };
    if !n.is_finite() {
        return Err(Error::Response("non-finite playback number"));
    }
    Ok(n)
}
pub fn online_candidate(song: &Value) -> Result<bool> {
    let p = &song["privilege"];
    if !js_truthy(p) {
        return Ok(true);
    }
    if !p.is_object() {
        return Err(Error::Response("invalid playback privilege"));
    }
    // M/21.A maps wire privilege.pl to its DB maxPlayBr. Native catalogue
    // metadata remains raw; project it before applying M/23.Z.
    let play = p.get("pl").unwrap_or(&p["maxPlayBr"]);
    let maximum = if js_truthy(play) { number(play)? } else { 0.0 };
    let flags = number(&p["flag"])?;
    let flags = flags.trunc().rem_euclid(4_294_967_296.0) as u32;
    Ok(maximum > 0.0 || js_truthy(&song["isOfflineDownOnline"]) || flags & (1 << 10) != 0)
}

pub fn identical_recording(song: &Value) -> Option<String> {
    let r = &song["noCopyrightRcmd"];
    (r["type"].as_i64() == Some(1))
        .then(|| crate::protocol::api::identifier(&r["songId"]).ok())
        .flatten()
}
