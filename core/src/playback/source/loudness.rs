use crate::library::{
    identity::fingerprint,
    matching::{MatchHint, file_metadata_hint},
};
use serde_json::{Value, json};
use std::path::Path;

/// This is local optional metadata, not authorization. An unreadable comment
/// disables loudness without preventing the decoder from opening valid audio.
pub(super) fn file_gain(path: &Path) -> (Value, bool) {
    let result = (|| {
        let before = fingerprint(path)?;
        let hint = file_metadata_hint(
            path,
            path.extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("ncm")),
        )?;
        if before != fingerprint(path)? {
            return Err(crate::Error::State("local loudness metadata changed"));
        }
        Ok(hint)
    })();
    match result {
        Ok(MatchHint::Song(value)) => (value["volumeDelta"].clone(), false),
        Ok(MatchHint::Absent) => (Value::Null, false),
        Ok(MatchHint::Invalid) | Err(_) => (Value::Null, true),
    }
}
pub(super) fn with_gain(mut info: Value, (gain, invalid): (Value, bool)) -> Value {
    info["volume_delta"] = gain;
    info["loudness_invalid"] = json!(invalid);
    info
}
