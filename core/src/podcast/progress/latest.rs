//! M/71 latestPlayVoiceMap: a committed local departure, keyed by native radio.
use super::program_identity;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::{StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KIND: &str = "podcast-latest-voice-v1";

pub(crate) struct Captured {
    radio_id: String,
    id: String,
    media_id: String,
    name: String,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LatestVoice {
    pub id: String,
    pub main_track_id: String,
    pub name: String,
    pub time: u64,
    pub duration: u64,
}

/// M3085 selects this one-shot breakpoint only from the current native
/// per-radio departure record, never from a renderer or remote history row.
#[derive(Clone)]
pub(crate) struct ShuffleAnchor {
    radio_id: String,
    record: LatestVoice,
}

impl ShuffleAnchor {
    pub(crate) fn capture(
        session: &AccountSession,
        token: &SessionToken,
        resource: &Value,
    ) -> Result<Option<Self>> {
        let Some(identity) = Captured::from_resource(resource) else {
            return Ok(None);
        };
        let store = session.persistent_store()?;
        session.with_token(token, |owner| {
            store.read_snapshot(|tx| {
                Ok(read(tx, owner, &identity.radio_id)?
                    .filter(|row| row.id == identity.id && row.main_track_id == identity.media_id)
                    .map(|record| Self {
                        radio_id: identity.radio_id,
                        record,
                    }))
            })
        })
    }

    pub(crate) fn radio_id(&self) -> &str {
        &self.radio_id
    }

    pub(crate) fn matches(&self, resource: &Value) -> bool {
        Captured::from_resource(resource).is_some_and(|identity| {
            identity.radio_id == self.radio_id
                && identity.id == self.record.id
                && identity.media_id == self.record.main_track_id
        })
    }

    pub(crate) fn require_current(&self, store: &StateStore, owner: &str) -> Result<()> {
        if store
            .read_snapshot(|tx| read(tx, owner, &self.radio_id))?
            .as_ref()
            != Some(&self.record)
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }

    pub(super) fn seconds(&self) -> f64 {
        self.record.time as f64
    }
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
