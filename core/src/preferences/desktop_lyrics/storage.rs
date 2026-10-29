use super::{DesktopLyricPreferences, Edit};
use crate::{Error, Result, storage::StateStore};
use serde_json::{Value, json};
const OWNER: &str = "local-device";
const KIND: &str = "desktop-lyric-preferences-v1";
fn damaged() -> Error {
    Error::Json(<serde_json::Error as serde::de::Error>::custom(
        "desktop lyric preference damaged",
    ))
}
fn current(value: Option<Value>, saved: Option<Value>) -> Result<DesktopLyricPreferences> {
    match (value, saved) {
        (None, None) => Ok(DesktopLyricPreferences::default()),
        (Some(value), Some(Value::Bool(true))) => {
            let value: DesktopLyricPreferences = serde_json::from_value(value)?;
            let id = uuid::Uuid::parse_str(&value.revision).map_err(|_| damaged())?;
            if id.hyphenated().to_string() != value.revision {
                return Err(damaged());
            }
            value.validate().map_err(|_| damaged())?;
            Ok(value)
        }
        _ => Err(damaged()),
    }
}
impl DesktopLyricPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )
        })
    }
    pub fn change(store: &StateStore, expected: &str, edit: Edit) -> Result<Self> {
        store.transaction(|tx| {
            let previous = current(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )?;
            if previous.revision != expected {
                return Err(Error::State("desktop lyric settings changed"));
            }
            let mut next = previous;
            next.apply(edit)?;
            next.revision = uuid::Uuid::new_v4().to_string();
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
                return Err(Error::State("desktop lyric settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                ..Self::default()
            };
            tx.put(
                OWNER,
                "desktop-lyric-recovery-v1",
                &next.revision,
                &json!({"current":parsed.ok().flatten(),"saved":marker.ok().flatten(),"current_raw":raw,"saved_raw":saved}),
            )?;
            tx.put(OWNER, KIND, "saved", &json!(true))?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}
