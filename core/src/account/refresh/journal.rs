use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KIND: &str = "login_refresh";
const KEY: &str = "jsk";
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Intent {
    pub id: String,
    pub pending: bool,
    pub before_rotation: Option<String>,
    pub last_success: Option<u64>,
}
pub(super) enum Begin {
    Started(Intent),
    NotDue,
    RecoveryRequired,
}

pub(super) fn load(store: &StateStore, uid: &str) -> Result<Option<Intent>> {
    store
        .get(uid, KIND, KEY)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(Error::from)
}

pub(super) fn begin(
    store: &StateStore,
    uid: &str,
    now: u64,
    force: bool,
    before_rotation: Option<String>,
) -> Result<Begin> {
    store.transaction(|tx| {
        let previous: Option<Intent> = tx
            .get(uid, KIND, KEY)?
            .map(serde_json::from_value)
            .transpose()?;
        if previous.as_ref().is_some_and(|p| p.pending) {
            return Ok(Begin::RecoveryRequired);
        }
        let last_success = previous.and_then(|p| p.last_success);
        // Official l && now-l < interval also skips after a backward clock
        // adjustment. Zero is false in the inspected JS.
        if !force && last_success.is_some_and(|last| last != 0 && now < last.saturating_add(86400))
        {
            return Ok(Begin::NotDue);
        }
        let intent = Intent {
            id: Uuid::new_v4().simple().to_string(),
            pending: true,
            before_rotation,
            last_success,
        };
        tx.put(uid, KIND, KEY, &serde_json::to_value(&intent)?)?;
        Ok(Begin::Started(intent))
    })
}

pub(super) fn complete(store: &StateStore, uid: &str, intent: &Intent, now: u64) -> Result<()> {
    finish(store, uid, intent, Some(now))
}

/// Only proven pre-transport failures can abandon the intent. Retain the
/// previous successful time; unknown HTTP outcomes must remain pending.
pub(super) fn not_sent(store: &StateStore, uid: &str, intent: &Intent) -> Result<()> {
    finish(store, uid, intent, intent.last_success)
}

fn finish(store: &StateStore, uid: &str, intent: &Intent, last_success: Option<u64>) -> Result<()> {
    store.transaction(|tx| {
        let current: Intent = tx
            .get(uid, KIND, KEY)?
            .map(serde_json::from_value)
            .transpose()?
            .ok_or(Error::StaleResponse)?;
        if !current.pending || current.id != intent.id {
            return Err(Error::StaleResponse);
        }
        let done = Intent {
            pending: false,
            last_success,
            ..current
        };
        tx.put(uid, KIND, KEY, &serde_json::to_value(done)?)
    })
}
