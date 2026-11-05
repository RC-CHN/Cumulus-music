//! M/71 latestPlayVoiceMap: a committed local departure, keyed by native radio.
use super::program_identity;
use crate::{Result, protocol::api::identifier, storage::StateTransaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KIND: &str = "podcast-latest-voice-v1";

pub(crate) struct Captured {
    radio_id: String,
    id: String,
    media_id: String,
    name: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LatestVoice {
    pub id: String,
    pub main_track_id: String,
    pub name: String,
    pub time: u64,
    pub duration: u64,
}

impl Captured {
    pub(crate) fn from_resource(resource: &Value) -> Option<Self> {
        if resource["download_task_id"].is_string() {
            return None;
        }
        let (voice, id, media_id) = program_identity(resource)?;
        Some(Self {
            radio_id: identifier(&voice["audio"]["id"]).ok()?,
            id,
            media_id,
            name: voice["name"].as_str().unwrap_or("").to_owned(),
        })
    }

    pub(crate) fn matches(&self, resource: &Value) -> bool {
        Self::from_resource(resource).is_some_and(|current| {
            self.radio_id == current.radio_id
                && self.id == current.id
                && self.media_id == current.media_id
        })
    }

    pub(crate) fn save(
        self,
        tx: &mut StateTransaction<'_>,
        owner: &str,
        position: f64,
        duration: f64,
        completed: bool,
    ) -> Result<()> {
        if !position.is_finite()
            || position < 0.0
            || !duration.is_finite()
            || duration <= 0.0
            || duration >= u64::MAX as f64 / 1000.0
        {
            return Ok(());
        }
        let time = if completed {
            duration
        } else {
            position.min(duration)
        };
        let row = LatestVoice {
            id: self.id,
            main_track_id: self.media_id,
            name: self.name,
            time: time.floor() as u64,
            duration: duration.floor() as u64,
        };
        tx.put(owner, KIND, &self.radio_id, &serde_json::to_value(row)?)
    }
}

pub(crate) fn read(
    tx: &StateTransaction<'_>,
    owner: &str,
    radio: &str,
) -> Result<Option<LatestVoice>> {
    Ok(tx
        .get(owner, KIND, radio)?
        .and_then(|raw| serde_json::from_value::<LatestVoice>(raw).ok())
        .filter(|row| {
            identifier(&Value::String(row.id.clone()))
                .as_ref()
                .is_ok_and(|id| id == &row.id)
                && identifier(&Value::String(row.main_track_id.clone()))
                    .as_ref()
                    .is_ok_and(|id| id == &row.main_track_id)
                && row.time <= row.duration
                && row.duration <= u64::MAX / 1000
        }))
}
