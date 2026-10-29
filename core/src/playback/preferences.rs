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
        store.transaction(|tx| {
            current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "revision")?.is_some(),
            )
        })
    }
    pub fn replace(store: &StateStore, expected: u64, settings: StartupSettings) -> Result<Self> {
        store.transaction(|tx| {
            let recorded_revision = tx.get(OWNER, KIND, "revision")?;
            let current = current(tx.get(OWNER, KIND, "current")?, recorded_revision.is_some())?;
            if current.revision != expected {
                return Err(Error::State("playback settings changed"));
            }
            let next = Self {
                revision: counter(recorded_revision)?
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
    /// another window's successful repair. Only a new installation has no records.
    pub fn repair(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            let recorded_revision = tx.get(OWNER, KIND, "revision")?;
            let text = tx.get_raw(OWNER, KIND, "current")?;
            let parsed = text.as_deref().map(serde_json::from_str).transpose();
            if let Ok(raw) = &parsed
                && current(raw.clone(), recorded_revision.is_some()).is_ok()
            {
                return Err(Error::State("playback settings changed"));
            }
            let raw = match parsed {
                Ok(Some(raw)) => raw,
                Ok(None) => serde_json::json!({"missing":true}),
                Err(_) => serde_json::json!({"current_raw":text}),
            };
            // Repair must not reuse a revision observed by an older window.
            // Keep the counter separately so malformed settings cannot erase it.
            let revision = counter(Some(
                recorded_revision.ok_or(Error::State("playback settings revision damaged"))?,
            ))?
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

fn current(value: Option<serde_json::Value>, previously_saved: bool) -> Result<StartupPreferences> {
    match value {
        Some(value) => Ok(serde_json::from_value(value)?),
        None if !previously_saved => Ok(StartupPreferences::default()),
        None => Err(Error::Json(
            <serde_json::Error as serde::de::Error>::custom(
                "saved playback startup preference is missing",
            ),
        )),
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
