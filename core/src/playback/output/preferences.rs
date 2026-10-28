use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};

const OWNER: &str = "local-device";
const KIND: &str = "audio-output-preferences-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StoredSelection")]
pub struct OutputSelection {
    id: String,
    description: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSelection {
    id: String,
    description: String,
}
impl TryFrom<StoredSelection> for OutputSelection {
    type Error = Error;
    fn try_from(value: StoredSelection) -> Result<Self> {
        Self::new(value.id, value.description)
    }
}
impl OutputSelection {
    /// Descriptions are copied from native enumeration, never rendered as HTML.
    pub fn new(id: String, description: String) -> Result<Self> {
        if id.is_empty()
            || id.len() > 4096
            || id.contains('\0')
            || description.len() > 16384
            || description.contains('\0')
        {
            return Err(Error::InvalidInput("invalid audio output selection"));
        }
        Ok(Self { id, description })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn description(&self) -> &str {
        &self.description
    }
}
impl Default for OutputSelection {
    fn default() -> Self {
        Self {
            id: "auto".into(),
            description: "系统默认".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPreferences {
    pub revision: u64,
    pub selection: OutputSelection,
}
impl OutputPreferences {
    pub fn read(store: &StateStore) -> Result<Self> {
        // Read both records in one SQLite statement. A deleted current record
        // after an earlier save is damage, not a new installation at revision 0.
        let rows = store.values(OWNER, KIND)?;
        current(rows.get("current").cloned(), rows.contains_key("revision"))
    }
    pub fn replace(store: &StateStore, expected: u64, selection: OutputSelection) -> Result<Self> {
        store.transaction(|tx| {
            let recorded_revision = tx.get(OWNER, KIND, "revision")?;
            let current = current(tx.get(OWNER, KIND, "current")?, recorded_revision.is_some())?;
            if current.revision != expected {
                return Err(Error::State("audio output settings changed"));
            }
            let next = Self {
                revision: counter(recorded_revision)?
                    .max(current.revision)
                    .checked_add(1)
                    .ok_or(Error::State("audio output revision exhausted"))?,
                selection,
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
                return Err(Error::State("audio output settings changed"));
            }
            let raw = raw.unwrap_or_else(|| serde_json::json!({"missing":true}));
            let revision = counter(recorded_revision)?
                .max(raw["revision"].as_u64().unwrap_or(0))
                .checked_add(1)
                .ok_or(Error::State("audio output revision exhausted"))?;
            tx.put(
                OWNER,
                "audio-output-recovery-v1",
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
fn current(value: Option<serde_json::Value>, previously_saved: bool) -> Result<OutputPreferences> {
    match value {
        Some(value) => Ok(serde_json::from_value(value)?),
        None if !previously_saved => Ok(OutputPreferences::default()),
        None => Err(Error::Json(
            <serde_json::Error as serde::de::Error>::custom(
                "saved audio output preference is missing",
            ),
        )),
    }
}
fn counter(value: Option<serde_json::Value>) -> Result<u64> {
    match value {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(Error::State("audio output revision damaged")),
    }
}
