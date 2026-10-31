//! Explicit local repair. A broken coordinator cannot be silently interpreted
//! as success or erase the current queue. Original bytes remain quarantined.
use super::*;
use crate::storage::StateStore;

pub struct IntelligenceRecovery {
    owner: String,
    queue_raw: Option<String>,
    session_raw: Option<String>,
    queue: QueueSnapshot,
    receipt: String,
}
impl IntelligenceRecovery {
    /// Native-only ticket; do not serialize its raw account data to a frontend.
    /// The caller must bind owner and later adoption to its account generation.
    pub fn inspect(store: &StateStore, owner: &str) -> Result<Option<Self>> {
        let (queue_raw, session_raw) = store.transaction(|tx| {
            Ok((
                tx.get_raw(owner, "playback", "queue")?,
                tx.get_raw(owner, KIND, KEY)?,
            ))
        })?;
        let mut raw: Value = queue_raw
            .as_ref()
            .map(|v| serde_json::from_str(v))
            .transpose()?
            .unwrap_or_else(|| json!({}));
        if raw.get("intelligence").is_none_or(Value::is_null) && session_raw.is_none() {
            return Ok(None);
        }
        raw.as_object_mut()
            .ok_or(Error::State("cannot repair malformed playback queue"))?
            .remove("intelligence");
        let queue = QueueSnapshot::load(Some(&raw))?;
        ordinary(&queue)?;
        for entry in &queue.items {
            crate::playback::PlaybackConstraint::from_resource(&entry.resource)?;
        }
        Ok(Some(Self {
            owner: owner.into(),
            queue_raw,
            session_raw,
            queue,
            receipt: Uuid::new_v4().simple().to_string(),
        }))
    }
    pub fn queue_len(&self) -> usize {
        self.queue.items.len()
    }
    /// Preserve media/order/selection/quality, convert only ownership to an
    /// ordinary queue, and retain a private copy for diagnosis. No HTTP/replay.
    pub fn adopt_ordinary(self, store: &StateStore) -> Result<QueueSnapshot> {
        store.transaction(|tx| {
            if tx.get_raw(&self.owner, "playback", "queue")? != self.queue_raw
                || tx.get_raw(&self.owner, KIND, KEY)? != self.session_raw
            {
                return Err(Error::QueueChanged);
            }
            tx.put(
                &self.owner,
                "playback-intelligence-quarantine-v1",
                &self.receipt,
                &json!({"queue":self.queue_raw,"session":self.session_raw}),
            )?;
            tx.put(
                &self.owner,
                "playback",
                "queue",
                &serde_json::to_value(&self.queue)?,
            )?;
            tx.delete(&self.owner, KIND, KEY)
        })?;
        Ok(self.queue)
    }
}
