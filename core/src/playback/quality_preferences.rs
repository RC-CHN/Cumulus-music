//! Account-local listening quality. Queue entries remember which preference
//! revision they were created under, so a later choice also applies to next.
use crate::{
    Error, Result,
    membership::Quality,
    storage::{StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const KIND: &str = "playback-quality-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityPreferences {
    pub revision: String,
    pub quality: Quality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trial: Option<QualityTrialBinding>,
}
/// Native provenance for a temporary preference. Frontends cannot choose the
/// fallback or manufacture a journal operation through an ordinary Quality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityTrialBinding {
    pub operation: String,
    pub right: crate::playback::quality::QualityRight,
    pub previous: Quality,
}
impl Default for QualityPreferences {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            quality: Quality::Standard,
            trial: None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QualityPreferencesSnapshot {
    pub preferences: QualityPreferences,
    pub damaged: bool,
}
impl QualityPreferencesSnapshot {
    pub(crate) fn read(store: &StateStore, owner: &str) -> Result<Self> {
        match QualityPreferences::read(store, owner) {
            Ok(preferences) => Ok(Self {
                preferences,
                damaged: false,
            }),
            Err(Error::Json(_)) => Ok(Self {
                damaged: true,
                ..Default::default()
            }),
            Err(error) => Err(error),
        }
    }
}
fn damaged() -> Error {
    Error::Json(<serde_json::Error as serde::de::Error>::custom(
        "listening quality preference damaged",
    ))
}
fn current(value: Option<Value>, saved: Option<Value>) -> Result<QualityPreferences> {
    match (value, saved) {
        (None, None) => Ok(QualityPreferences::default()),
        (Some(value), Some(Value::Bool(true))) => {
            let value: QualityPreferences = serde_json::from_value(value)?;
            let id = uuid::Uuid::parse_str(&value.revision).map_err(|_| damaged())?;
            if id.hyphenated().to_string() != value.revision {
                return Err(damaged());
            }
            if let Some(trial) = &value.trial
                && (trial.right.quality() != value.quality
                    || !uuid::Uuid::parse_str(&trial.operation)
                        .is_ok_and(|id| id.simple().to_string() == trial.operation))
            {
                return Err(damaged());
            }
            Ok(value)
        }
        _ => Err(damaged()),
    }
}
impl QualityPreferences {
    pub(crate) fn clamp_member_default(
        tx: &mut StateTransaction<'_>,
        owner: &str,
        ceiling: Quality,
    ) -> Result<()> {
        let current = match Self::read_in(tx, owner) {
            Ok(preferences) => preferences,
            Err(Error::Json(_)) => return Ok(()), // Preserve the damaged record.
            Err(error) => return Err(error),
        };
        let preferred = current
            .trial
            .as_ref()
            .map_or(current.quality, |trial| trial.previous);
        if preferred.bitrate() <= ceiling.bitrate() {
            return Ok(());
        }
        let mut next = Self::fresh(ceiling);
        if let Some(mut trial) = current.trial {
            // This local cap cannot revoke a separately authorized active
            // trial. Its ordinary preference upon expiry still obeys M/116.
            trial.previous = ceiling;
            next.quality = current.quality;
            next.trial = Some(trial);
        }
        // A persisted binding is kept byte-for-byte apart from its default
        // fallback; this cannot create, revive or extend a trial authorization.
        Self::write_in(tx, owner, &next)
    }

    pub fn read(store: &StateStore, owner: &str) -> Result<Self> {
        store.transaction(|tx| Self::read_in(tx, owner))
    }
    fn read_in(tx: &StateTransaction<'_>, owner: &str) -> Result<Self> {
        current(
            tx.get(owner, KIND, "current")?,
            tx.get(owner, KIND, "saved")?,
        )
    }
    pub fn replace(
        store: &StateStore,
        owner: &str,
        expected: &str,
        quality: Quality,
    ) -> Result<Self> {
        let next = Self::fresh(quality);
        store.transaction(|tx| Self::replace_in(tx, owner, expected, &next))?;
        Ok(next)
    }
    pub(crate) fn fresh(quality: Quality) -> Self {
        Self {
            revision: uuid::Uuid::new_v4().to_string(),
            quality,
            trial: None,
        }
    }
    pub(crate) fn replace_in(
        tx: &mut StateTransaction<'_>,
        owner: &str,
        expected: &str,
        next: &Self,
    ) -> Result<()> {
        let previous = Self::read_in(tx, owner)?;
        if previous.revision != expected {
            return Err(Error::State("listening quality settings changed"));
        }
        if let Some(trial) = &next.trial {
            if trial.right.quality() != next.quality {
                return Err(damaged());
            }
            crate::playback::quality::trials::QualityTrials::require_active_in(
                tx,
                owner,
                trial.right,
                &trial.operation,
            )?;
        }
        Self::write_in(tx, owner, next)
    }
    fn write_in(tx: &mut StateTransaction<'_>, owner: &str, value: &Self) -> Result<()> {
        tx.put(owner, KIND, "saved", &json!(true))?;
        tx.put(owner, KIND, "current", &serde_json::to_value(value)?)
    }
    pub(crate) fn fresh_trial(quality: Quality, trial: QualityTrialBinding) -> Self {
        Self {
            trial: Some(trial),
            ..Self::fresh(quality)
        }
    }
    /// Only a freshly observed expiry may undo this exact trial preference.
    /// A later manual choice, another trial or account switch is never rolled back.
    pub(crate) fn restore_trial(store: &StateStore, owner: &str, expected: &Self) -> Result<bool> {
        let Some(trial) = &expected.trial else {
            return Ok(false);
        };
        store.transaction(|tx| {
            let latest = Self::read_in(tx, owner)?;
            if latest != *expected {
                return Ok(false);
            }
            crate::playback::quality::trials::QualityTrials::require_expired_in(
                tx,
                owner,
                trial.right,
                &trial.operation,
            )?;
            Self::replace_in(tx, owner, &expected.revision, &Self::fresh(trial.previous))?;
            Ok(true)
        })
    }
    /// Explicit recovery only. Keep the exact malformed data for local repair;
    /// a fresh token prevents a stale menu from overwriting the repaired value.
    pub fn repair(store: &StateStore, owner: &str) -> Result<Self> {
        store.transaction(|tx| {
            let raw = tx.get_raw(owner, KIND, "current")?;
            let saved = tx.get_raw(owner, KIND, "saved")?;
            let parsed = raw
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose();
            let marker = saved
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose();
            if let (Ok(value), Ok(marker)) = (&parsed, &marker)
                && current(value.clone(), marker.clone()).is_ok()
            {
                return Err(Error::State("listening quality settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                ..Self::default()
            };
            tx.put(
                owner,
                "playback-quality-recovery-v1",
                &next.revision,
                &json!({"current_raw":raw,"saved_raw":saved}),
            )?;
            tx.put(owner, KIND, "saved", &json!(true))?;
            tx.put(owner, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::quality::QualityRight;

    #[test]
    fn member_cap_preserves_a_trial_binding_and_only_caps_its_ordinary_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path()).unwrap();
        let old = QualityPreferences::fresh_trial(
            Quality::Vivid,
            QualityTrialBinding {
                operation: uuid::Uuid::new_v4().simple().to_string(),
                right: QualityRight::Vivid,
                previous: Quality::Sky,
            },
        );
        let low = QualityPreferences::replace(&store, "2", "initial", Quality::Higher).unwrap();
        store
            .transaction(|tx| QualityPreferences::write_in(tx, "1", &old))
            .unwrap();
        store
            .transaction(|tx| {
                QualityPreferences::clamp_member_default(tx, "1", Quality::JyEffect)?;
                QualityPreferences::clamp_member_default(tx, "2", Quality::ExHigh)
            })
            .unwrap();
        let next = QualityPreferences::read(&store, "1").unwrap();
        assert_ne!(next.revision, old.revision);
        assert_eq!(next.quality, Quality::Vivid);
        let old = old.trial.unwrap();
        let next = next.trial.unwrap();
        assert_eq!((next.operation, next.right), (old.operation, old.right));
        assert_eq!(next.previous, Quality::JyEffect);
        assert_eq!(QualityPreferences::read(&store, "2").unwrap(), low);
    }
}
