use super::EqualizerSettings;
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};

const KIND: &str = "equalizer-preferences-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EqualizerPreset {
    BuiltIn { index: u8 },
    Manual,
    Saved,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqualizerPreferences {
    pub revision: u64,
    pub settings: EqualizerSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<EqualizerPreset>,
    // Existing records use the settings revision as their DSP revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dsp_revision: Option<u64>,
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
    pub(crate) fn dsp_revision(&self) -> u64 {
        self.dsp_revision.unwrap_or(self.revision)
    }

    /// Desired configuration is durable before touching the audio adapter.
    /// The actor reports application separately and can retry after restart.
    pub fn replace(
        store: &StateStore,
        owner: &str,
        expected: u64,
        settings: EqualizerSettings,
        preset: Option<EqualizerPreset>,
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
            if matches!(preset, Some(EqualizerPreset::BuiltIn { index: 24.. }))
                || matches!(preset, Some(EqualizerPreset::Saved))
                    && current.saved_custom.map(|saved| *saved.gains()) != Some(*settings.gains())
            {
                return Err(Error::InvalidInput("invalid equalizer preset selection"));
            }
            let identity_only =
                preset.is_some() && preset != current.preset && settings == current.settings;
            let next = Self {
                revision: current
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("equalizer revision exhausted"))?,
                settings,
                preset,
                dsp_revision: identity_only.then(|| current.dsp_revision()),
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
            let manual = current.preset == Some(EqualizerPreset::Manual);
            if current.saved_custom == custom && !manual {
                return Ok(current);
            }
            current.saved_custom = custom;
            if manual {
                let revision = current.dsp_revision();
                current.revision = current
                    .revision
                    .checked_add(1)
                    .ok_or(Error::State("equalizer revision exhausted"))?;
                current.dsp_revision = Some(revision);
                current.preset = Some(EqualizerPreset::Saved);
            }
            tx.put(owner, KIND, "current", &serde_json::to_value(current)?)?;
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
                ..Self::default()
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
