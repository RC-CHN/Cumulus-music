//! Device-local system media preference. Opaque CAS tokens are never reused,
//! including after explicit repair of damaged content or its saved marker.
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
const OWNER: &str = "local-device";
const KIND: &str = "system-media-preferences-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemMediaPreferences {
    pub revision: String,
    pub enabled: bool,
}
impl Default for SystemMediaPreferences {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            enabled: true,
        }
    }
}
fn damaged() -> Error {
    Error::Json(<serde_json::Error as serde::de::Error>::custom(
        "system media preference damaged",
    ))
}
fn current(value: Option<Value>, saved: Option<Value>) -> Result<SystemMediaPreferences> {
    match (value, saved) {
        (None, None) => Ok(SystemMediaPreferences::default()),
        (Some(value), Some(Value::Bool(true))) => {
            let value: SystemMediaPreferences = serde_json::from_value(value)?;
            let id = uuid::Uuid::parse_str(&value.revision).map_err(|_| damaged())?;
            if id.hyphenated().to_string() != value.revision {
                return Err(damaged());
            }
            Ok(value)
        }
        _ => Err(damaged()),
    }
}
impl SystemMediaPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        let rows = store.values(OWNER, KIND)?;
        current(rows.get("current").cloned(), rows.get("saved").cloned())
    }
    pub fn replace(store: &StateStore, expected: &str, enabled: bool) -> Result<Self> {
        store.transaction(|tx| {
            let previous = current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )?;
            if previous.revision != expected {
                return Err(Error::State("system media settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                enabled,
            };
            tx.put(OWNER, KIND, "saved", &json!(true))?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
    pub fn repair(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            let raw = tx.get(OWNER, KIND, "current")?;
            let saved = tx.get(OWNER, KIND, "saved")?;
            if current(raw.clone(), saved.clone()).is_ok() {
                return Err(Error::State("system media settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                ..Self::default()
            };
            tx.put(
                OWNER,
                "system-media-recovery-v1",
                &next.revision,
                &json!({"current":raw,"saved":saved}),
            )?;
            tx.put(OWNER, KIND, "saved", &json!(true))?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}
