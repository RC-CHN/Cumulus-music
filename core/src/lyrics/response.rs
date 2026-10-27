//! Official M/21.s/m transport-to-lyric conversion. Cached values are already
//! normalized; never divide cached offsets by 1000 a second time.
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy},
};
use serde_json::{Map, Value, json};

pub(super) const KINDS: [&str; 4] = ["lrc", "yrc", "tlyric", "romalrc"];

pub fn normalize_song_response(raw: &Value) -> Result<Value> {
    if !raw.is_object() {
        return Err(Error::Response("lyric response must be an object"));
    }
    let mut out = Map::new();
    for (to, from) in [
        ("code", "code"),
        ("canUploadLyric", "sgc"),
        ("canUploadTranslation", "sfy"),
        ("askForTranslation", "qfy"),
    ] {
        if let Some(value) = raw.get(from) {
            out.insert(to.into(), value.clone());
        }
    }
    for (to, from) in [
        ("lyricContributor", "lyricUser"),
        ("translationContributor", "transUser"),
    ] {
        if let Some(value) = raw.get(from).filter(|v| js_truthy(v)) {
            let mut value = value
                .as_object()
                .ok_or(Error::Response("invalid lyric contributor"))?
                .clone();
            for key in ["id", "userid"] {
                let text = value
                    .get(key)
                    .map(js_string)
                    .transpose()?
                    .unwrap_or_else(|| "undefined".into());
                value.insert(key.into(), json!(text));
            }
            out.insert(to.into(), Value::Object(value));
        }
    }
    for key in ["lrc", "yrc", "tlyric", "romalrc", "yromalrc", "ytlrc"] {
        if let Some(value) = raw.get(key).filter(|v| js_truthy(v)) {
            let mut value = value
                .as_object()
                .ok_or(Error::Response("invalid lyric section"))?
                .clone();
            let offset = match value.get("offset").filter(|v| js_truthy(v)) {
                Some(value) => {
                    value
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or(Error::Response("invalid lyric offset"))?
                        / 1000.0
                }
                None => 0.0,
            };
            value.insert("offset".into(), json!(offset));
            if !KINDS.contains(&key) || value.get("lyric").is_some_and(js_truthy) {
                out.insert(key.into(), Value::Object(value));
            }
        }
    }
    let no_lyric = !KINDS.iter().any(|key| out.get(*key).is_some_and(js_truthy));
    out.insert("noLyric".into(), json!(no_lyric));
    out.insert("isCloudLyric".into(), json!(false));
    Ok(Value::Object(out))
}

pub fn normalize_cloud_response(raw: &Value) -> Result<Value> {
    if !raw.is_object() {
        return Err(Error::Response("cloud lyric response must be an object"));
    }
    let mut lrc = json!({"version":-1,"offset":0});
    if let Some(text) = raw.get("lrc") {
        if !text.is_string() {
            return Err(Error::Response("cloud lyric must be a string"));
        }
        lrc["lyric"] = text.clone();
    }
    let mut out = json!({"isCloudLyric":true,"canUploadLyric":false,"canUploadTranslation":false,"askForTranslation":false,"noLyric":true,"lrc":lrc});
    if let Some(code) = raw.get("code") {
        out["code"] = code.clone();
    }
    Ok(out)
}

pub(super) fn local_response(text: &str) -> Value {
    json!({"code":200,"noLyric":false,"lrc":{"version":-1,"lyric":text},"canUploadLyric":false,"askForTranslation":false,"canUploadTranslation":false})
}

pub(super) fn merge(cached: Option<Value>, incoming: Option<Value>) -> Value {
    let mut out = cached
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if let Some(incoming) = incoming.and_then(|v| v.as_object().cloned()) {
        out.extend(incoming);
    }
    let no_lyric = !KINDS.iter().any(|key| out.get(*key).is_some_and(js_truthy));
    out.insert("noLyric".into(), json!(no_lyric));
    Value::Object(out)
}
