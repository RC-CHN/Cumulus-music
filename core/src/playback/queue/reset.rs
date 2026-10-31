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
                let mode = if current
                    .intelligence
                    .as_ref()
                    .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
                {
                    let session: crate::playback::intelligence::IntelligenceSession =
                        serde_json::from_value(
                            tx.get(
                                &self.owner,
                                crate::playback::intelligence::KIND,
                                crate::playback::intelligence::KEY,
                            )?
                            .ok_or(Error::State("missing intelligence session"))?,
                        )?;
                    session.validate(&current)?;
                    session.last_mode
                } else if current.fm_source.is_some()
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
                tx.delete(
                    &self.owner,
                    crate::playback::intelligence::KIND,
                    crate::playback::intelligence::KEY,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::intelligence::{IntelligenceRequest, IntelligenceStart},
        playback::{
            PlaybackMode, PlaybackQueue,
            intelligence::{IntelligenceAction, IntelligenceBegin},
        },
    };
    use std::sync::Arc;

    #[test]
    fn login_policy_clear_restores_the_mode_before_ai_and_removes_its_backup_atomically() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "9").unwrap();
        queue
            .replace(
                vec![json!({"id":7}), json!({"id":8})],
                0,
                json!({"kind":"playlist","id":"31"}),
            )
            .unwrap();
        queue.set_mode(PlaybackMode::Random).unwrap();
        let liked = queue.snapshot();
        queue
            .intelligence_transition(IntelligenceAction::Begin(Box::new(IntelligenceBegin {
                operation_id: None,
                request: IntelligenceRequest {
                    playlist_id: "31".into(),
                    song_id: "7".into(),
                    start_music_id: "7".into(),
                    start: IntelligenceStart::PlayAll,
                    count: 2,
                },
                seed_resource: json!({"id":7}),
                seed_source: json!({"kind":"playlist","id":"31"}),
                liked,
                preserve_current_queue: true,
                liked_metadata: vec![],
            })))
            .unwrap();
        assert_eq!(queue.mode(), PlaybackMode::OrderLoop);
        let plan = QueueResetPlan::capture(&store, "9").unwrap();
        plan.apply(&store).unwrap();
        assert!(queue.reload().unwrap());
        assert!(queue.is_empty());
        assert_eq!(queue.mode(), PlaybackMode::Random);
        assert!(queue.intelligence_context().is_none());
        assert!(
            store
                .get(
                    "9",
                    crate::playback::intelligence::KIND,
                    crate::playback::intelligence::KEY
                )
                .unwrap()
                .is_none()
        );
        plan.apply(&store).unwrap();
        assert!(!queue.reload().unwrap());
    }
}
