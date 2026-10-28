//! Device-local official close behavior. Platform tray availability is supplied
//! by the native host; unavailable or damaged state must never hide the window.
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
const OWNER: &str = "local-device";
const KIND: &str = "window-behavior-preferences-v1";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    #[default]
    Tray,
    Exit,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowSettings {
    pub close_behavior: CloseBehavior,
    pub remember_choice: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePlan {
    Ask,
    Hide,
    Exit,
}
impl WindowSettings {
    pub fn plan(self, tray_available: bool) -> ClosePlan {
        if !self.remember_choice {
            return ClosePlan::Ask;
        }
        match self.close_behavior {
            CloseBehavior::Exit => ClosePlan::Exit,
            CloseBehavior::Tray if tray_available => ClosePlan::Hide,
            CloseBehavior::Tray => ClosePlan::Ask,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowPreferences {
    pub revision: u64,
    pub settings: WindowSettings,
}
impl WindowPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        // Read both records in one SQLite statement. A deleted current record
        // after an earlier save is damage, not a new installation at revision 0.
        let rows = store.values(OWNER, KIND)?;
        current(rows.get("current").cloned(), rows.contains_key("revision"))
    }
    pub fn replace(store: &StateStore, expected: u64, settings: WindowSettings) -> Result<Self> {
        store.transaction(|tx| {
            let recorded_revision = tx.get(OWNER, KIND, "revision")?;
            let current = current(tx.get(OWNER, KIND, "current")?, recorded_revision.is_some())?;
            if current.revision != expected {
                return Err(Error::State("window behavior settings changed"));
            }
            let next = Self {
                revision: counter(recorded_revision)?
                    .max(current.revision)
                    .checked_add(1)
                    .ok_or(Error::State("window behavior revision exhausted"))?,
                settings,
            };
            tx.put(OWNER, KIND, "revision", &next.revision.into())?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
    /// Explicit repair backs up malformed content. A separate counter prevents
    /// an old window's CAS from becoming valid again after settings recovery.
    pub fn repair(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            let recorded_revision = tx.get(OWNER, KIND, "revision")?;
            let raw = tx.get(OWNER, KIND, "current")?;
            if current(raw.clone(), recorded_revision.is_some()).is_ok() {
                return Err(Error::State("window behavior settings changed"));
            }
            let raw = raw.unwrap_or_else(|| serde_json::json!({"missing":true}));
            let revision = counter(recorded_revision)?
                .max(raw["revision"].as_u64().unwrap_or(0))
                .checked_add(1)
                .ok_or(Error::State("window behavior revision exhausted"))?;
            tx.put(
                OWNER,
                "window-behavior-recovery-v1",
                &uuid::Uuid::new_v4().to_string(),
                &raw,
            )?;
            let next = Self {
                revision,
                ..Self::default()
            };
            tx.put(OWNER, KIND, "revision", &revision.into())?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}
fn current(value: Option<serde_json::Value>, previously_saved: bool) -> Result<WindowPreferences> {
    match value {
        Some(value) => Ok(serde_json::from_value(value)?),
        None if !previously_saved => Ok(WindowPreferences::default()),
        None => Err(Error::Json(
            <serde_json::Error as serde::de::Error>::custom(
                "saved window behavior preference is missing",
            ),
        )),
    }
}
fn counter(value: Option<serde_json::Value>) -> Result<u64> {
    match value {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(Error::State("window behavior revision damaged")),
    }
}
