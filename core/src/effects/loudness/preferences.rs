//! Device-local loudness preference. Opaque CAS tokens are never reused,
//! including after explicit repair of damaged content or its saved marker.
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
const OWNER: &str = "local-device";
const KIND: &str = "loudness-preferences-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoudnessPreferences {
    pub revision: String,
    pub enabled: bool,
}
impl Default for LoudnessPreferences {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            enabled: false,
        }
    }
}
fn damaged() -> Error {
    Error::Json(<serde_json::Error as serde::de::Error>::custom(
        "loudness preference damaged",
    ))
}
fn current(value: Option<Value>, saved: Option<Value>) -> Result<LoudnessPreferences> {
    match (value, saved) {
        (None, None) => Ok(LoudnessPreferences::default()),
        (Some(value), Some(Value::Bool(true))) => {
            let value: LoudnessPreferences = serde_json::from_value(value)?;
            let id = uuid::Uuid::parse_str(&value.revision).map_err(|_| damaged())?;
            if id.hyphenated().to_string() != value.revision {
                return Err(damaged());
            }
            Ok(value)
        }
        _ => Err(damaged()),
    }
}
impl LoudnessPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )
        })
    }
    pub fn replace(store: &StateStore, expected: &str, enabled: bool) -> Result<Self> {
        store.transaction(|tx| {
            let previous = current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )?;
            if previous.revision != expected {
                return Err(Error::State("loudness settings changed"));
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
            let raw = tx.get_raw(OWNER, KIND, "current")?;
            let saved = tx.get_raw(OWNER, KIND, "saved")?;
            let parsed = raw.as_deref().map(serde_json::from_str::<Value>).transpose();
            let marker = saved.as_deref().map(serde_json::from_str::<Value>).transpose();
            if let (Ok(value), Ok(marker)) = (&parsed, &marker)
                && current(value.clone(), marker.clone()).is_ok() {
                return Err(Error::State("loudness settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                ..Self::default()
            };
            tx.put(
                OWNER,
                "loudness-recovery-v1",
                &next.revision,
                &json!({"current":parsed.ok().flatten(),"saved":marker.ok().flatten(),"current_raw":raw,"saved_raw":saved}),
            )?;
            tx.put(OWNER, KIND, "saved", &json!(true))?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}
