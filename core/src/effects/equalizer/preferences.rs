use super::EqualizerSettings;
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};

const KIND: &str = "equalizer-preferences-v1";
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqualizerPreferences {
    pub revision: u64,
    pub settings: EqualizerSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_custom: Option<EqualizerSettings>,
}
impl EqualizerPreferences {
    pub fn read(store: &StateStore, owner: &str) -> Result<Self> {
        store
            .get(owner, KIND, "current")?
            .map(serde_json::from_value)
            .transpose()
            .map(|v| v.unwrap_or_default())
            .map_err(Into::into)
    }
    /// Desired configuration is durable before touching the audio adapter.
    /// The actor reports application separately and can retry after restart.
    pub fn replace(
        store: &StateStore,
        owner: &str,
        expected: u64,
        settings: EqualizerSettings,
    ) -> Result<Self> {
        store.transaction(|tx| {
            let current: Self = tx
                .get(owner, KIND, "current")?
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default();
            if current.revision != expected {
                return Err(Error::State("equalizer settings changed"));
            }
            let next = Self {
                revision: current
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("equalizer revision exhausted"))?,
                settings,
                saved_custom: current.saved_custom,
            };
            tx.put(owner, KIND, "current", &serde_json::to_value(next)?)?;
            Ok(next)
        })
    }
    /// Copy the acknowledged curve without changing the active settings or DSP revision.
    pub fn save_custom(store: &StateStore, owner: &str, expected: u64) -> Result<Self> {
        store.transaction(|tx| {
            let mut current: Self = tx
                .get(owner, KIND, "current")?
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default();
            if current.revision != expected {
                return Err(Error::State("equalizer settings changed"));
            }
            let custom = Some(EqualizerSettings::new(true, *current.settings.gains())?);
            if current.saved_custom != custom {
                current.saved_custom = custom;
                tx.put(owner, KIND, "current", &serde_json::to_value(current)?)?;
            }
            Ok(current)
        })
    }
    /// Explicit local repair only. Keep the corrupt value in the same account
    /// for recovery; refuse to overwrite a concurrently repaired valid record.
    pub fn repair(store: &StateStore, owner: &str) -> Result<Self> {
        store.transaction(|tx| {
            let raw = tx
                .get(owner, KIND, "current")?
                .ok_or(Error::State("equalizer settings changed"))?;
            if serde_json::from_value::<Self>(raw.clone()).is_ok() {
                return Err(Error::State("equalizer settings changed"));
            }
            let key = uuid::Uuid::new_v4().simple().to_string();
            tx.put(owner, "equalizer-recovery-v1", &key, &raw)?;
            let next = Self {
                revision: 1,
                settings: EqualizerSettings::default(),
                saved_custom: None,
            };
            tx.put(owner, KIND, "current", &serde_json::to_value(next)?)?;
            Ok(next)
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EqualizerStatus {
    #[default]
    Pending,
    Unavailable,
    Configured,
    Processing,
    Failed,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EqualizerSnapshot {
    pub preferences: EqualizerPreferences,
    pub status: EqualizerStatus,
    pub damaged: bool,
}
