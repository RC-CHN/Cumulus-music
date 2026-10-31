//! Idempotent local queue clearing requested by the legacy login policy.
//! The plan is committed with credential invalidation; completion is committed
//! with the queue in its separate store. It never deletes media or playlists.
use super::{FM_BACKUP_POSITION, FM_BACKUP_QUEUE, QueueSnapshot};
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct QueueResetPlan {
    id: String,
    owner: String,
    expected_queue: String,
}
fn digest(value: Option<Value>) -> Result<String> {
    let mut value = value.unwrap_or(Value::Null);
    value.sort_all_objects();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}
impl QueueResetPlan {
    pub(crate) fn capture(store: &StateStore, owner: &str) -> Result<Self> {
        let plan = Self {
            id: Uuid::new_v4().simple().to_string(),
            owner: owner.into(),
            expected_queue: digest(store.get(owner, "playback", "queue")?)?,
        };
        plan.validate()?;
        Ok(plan)
    }
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if Uuid::parse_str(&self.id).is_err()
            || self.owner.len() > 4096
            || (self.owner != "local-guest"
                && crate::protocol::api::identifier(&json!(self.owner)).is_err())
            || self.expected_queue.len() != 64
            || !self.expected_queue.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::State("invalid login queue reset plan"));
        }
        Ok(())
    }
    pub(crate) fn apply(&self, store: &StateStore) -> Result<()> {
        self.validate()?;
        store.transaction(|tx| {
            if let Some(receipt) = tx.get(&self.owner, "login-queue-reset-v1", &self.id)? {
                if receipt != json!("cleared") && receipt != json!("superseded") {
                    return Err(Error::State("invalid login queue reset receipt"));
                }
                return Ok(());
            }
            let previous = tx.get(&self.owner, "playback", "queue")?;
            let outcome = if digest(previous.clone())? == self.expected_queue {
                let current = QueueSnapshot::load(previous.as_ref())?;
                let mode = if current.fm_source.is_some()
                    || current.items.iter().any(|item| item.source["kind"] == "fm")
                {
                    match tx.get(&self.owner, "playback", FM_BACKUP_QUEUE)? {
                        Some(backup) => QueueSnapshot::load(Some(&backup))?.mode,
                        None => current.mode,
                    }
                } else {
                    current.mode
                };
                let empty = QueueSnapshot {
                    mode,
                    ..Default::default()
                };
                tx.put(
                    &self.owner,
                    "playback",
                    "queue",
                    &serde_json::to_value(empty)?,
                )?;
                for key in ["position", FM_BACKUP_QUEUE, FM_BACKUP_POSITION] {
                    tx.delete(&self.owner, "playback", key)?;
                }
                for key in ["source", "failure"] {
                    tx.delete(&self.owner, "fm-player-v1", key)?;
                }
                "cleared"
            } else {
                // After a crash another session may have installed a newer
                // queue. An old expiry must not erase that later work.
                "superseded"
            };
            tx.put(
                &self.owner,
                "login-queue-reset-v1",
                &self.id,
                &json!(outcome),
            )
        })
    }
}
