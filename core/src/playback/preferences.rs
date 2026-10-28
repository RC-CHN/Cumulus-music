//! Device-local startup policy. Queue contents and saved clocks remain scoped
//! to their account; these two official desktop preferences belong to the app.
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};

const OWNER: &str = "local-device";
const KIND: &str = "playback-startup-preferences-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupSettings {
    pub auto_play: bool,
    pub remember_position: bool,
}
impl Default for StartupSettings {
    fn default() -> Self {
        Self {
            auto_play: false,
            remember_position: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupPreferences {
    pub revision: u64,
    pub settings: StartupSettings,
}
impl StartupPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        store
            .get(OWNER, KIND, "current")?
            .map(serde_json::from_value)
            .transpose()
            .map(|value| value.unwrap_or_default())
            .map_err(Into::into)
    }
    pub fn replace(store: &StateStore, expected: u64, settings: StartupSettings) -> Result<Self> {
        store.transaction(|tx| {
            let current: Self = tx
                .get(OWNER, KIND, "current")?
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default();
            if current.revision != expected {
                return Err(Error::State("playback settings changed"));
            }
            let next = Self {
                revision: counter(tx.get(OWNER, KIND, "revision")?)?
                    .max(current.revision)
                    .checked_add(1)
                    .ok_or(Error::State("playback settings revision exhausted"))?,
                settings,
            };
            tx.put(OWNER, KIND, "revision", &next.revision.into())?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(next)?)?;
            Ok(next)
        })
    }
    /// Explicit repair preserves the damaged record and refuses to overwrite
    /// another window's successful repair. A missing record is a valid default.
    pub fn repair(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            let raw = tx
                .get(OWNER, KIND, "current")?
                .ok_or(Error::State("playback settings changed"))?;
            if serde_json::from_value::<Self>(raw.clone()).is_ok() {
                return Err(Error::State("playback settings changed"));
            }
            // Repair must not reuse a revision observed by an older window.
            // Keep the counter separately so malformed settings cannot erase it.
            let revision = counter(tx.get(OWNER, KIND, "revision")?)?
                .max(raw["revision"].as_u64().unwrap_or(0))
                .checked_add(1)
                .ok_or(Error::State("playback settings revision exhausted"))?;
            tx.put(
                OWNER,
                "playback-startup-recovery-v1",
                &uuid::Uuid::new_v4().to_string(),
                &raw,
            )?;
            let next = Self {
                revision,
                ..Self::default()
            };
            tx.put(OWNER, KIND, "revision", &revision.into())?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(next)?)?;
            Ok(next)
        })
    }
}

fn counter(value: Option<serde_json::Value>) -> Result<u64> {
    match value {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(Error::State("playback settings revision damaged")),
    }
}
