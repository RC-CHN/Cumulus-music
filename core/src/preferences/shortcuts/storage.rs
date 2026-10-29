use super::{ShortcutSettings, damaged};
use crate::{Error, Result, storage::StateStore};
use serde_json::{Value, json};
const OWNER: &str = "local-device";
const KIND: &str = "shortcut-settings-v1";
fn decode(current: Option<Value>, saved: Option<Value>) -> Result<ShortcutSettings> {
    match (current, saved) {
        (None, None) => Ok(ShortcutSettings::default()),
        (Some(raw), Some(Value::Bool(true))) => {
            let value: ShortcutSettings = serde_json::from_value(raw)?;
            let uuid = uuid::Uuid::parse_str(&value.revision).map_err(|_| damaged())?;
            if uuid.hyphenated().to_string() != value.revision {
                return Err(damaged());
            }
            value.validate()?;
            Ok(value)
        }
        _ => Err(damaged()),
    }
}
impl ShortcutSettings {
    pub fn read(store: &StateStore) -> Result<Self> {
        store.transaction(|tx| {
            decode(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )
        })
    }
    /// Compare and replace the entire validated map in one SQLite transaction.
    /// A committed map is intent, not proof that any OS registration succeeded.
    pub fn save(&self, store: &StateStore) -> Result<Self> {
        self.validate()?;
        store.transaction(|tx| {
            let prior = decode(
                tx.get(OWNER, KIND, "current")?,
                tx.get(OWNER, KIND, "saved")?,
            )?;
            if prior.revision != self.revision {
                return Err(Error::State("shortcut settings changed"));
            }
            let mut next = self.clone();
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
                && decode(value.clone(), marker.clone()).is_ok() {
                return Err(Error::State("shortcut settings changed"));
            }
            let next = Self {
                revision: uuid::Uuid::new_v4().to_string(),
                ..Self::default()
            };
            tx.put(
                OWNER,
                "shortcut-recovery-v1",
                &next.revision,
                &json!({"current":parsed.ok().flatten(),"saved":marker.ok().flatten(),"current_raw":raw,"saved_raw":saved}),
            )?;
            tx.put(OWNER, KIND, "saved", &json!(true))?;
            tx.put(OWNER, KIND, "current", &serde_json::to_value(&next)?)?;
            Ok(next)
        })
    }
}
