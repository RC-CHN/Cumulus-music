//! Forgetting a list entry keeps its existing local queue playable, with the
//! same account, entitlement and integrity checks. This ledger stays native.
use super::{OfflineCheck, OfflineLibrary, OfflineRecord};
use crate::{
    Error, Result,
    account::SessionToken,
    membership::{OfflineAccess, OfflineSource, offline_access},
    playback::{FM_BACKUP_QUEUE, QueueSnapshot, intelligence},
    storage::StateTransaction,
};
use serde_json::json;

pub(crate) fn detach_download_queue(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    record: &OfflineRecord,
) -> Result<()> {
    for key in ["queue", FM_BACKUP_QUEUE] {
        let Some(value) = tx.get(owner, "playback", key)? else {
            continue;
        };
        let mut queue = QueueSnapshot::load(Some(&value))?;
        if key == "queue" && queue.intelligence.is_some() {
            let mut session: intelligence::IntelligenceSession = serde_json::from_value(
                tx.get(owner, intelligence::KIND, intelligence::KEY)?
                    .ok_or(Error::State("missing intelligence session"))?,
            )?;
            session.validate(&queue)?;
            if detach_queue_entries(&mut session.backup, owner, record)? {
                session.revision = uuid::Uuid::new_v4().to_string();
                session.validate(&queue)?;
                tx.put(
                    owner,
                    intelligence::KIND,
                    intelligence::KEY,
                    &serde_json::to_value(session)?,
                )?;
            }
        }
        if detach_queue_entries(&mut queue, owner, record)? {
            tx.put(owner, "playback", key, &serde_json::to_value(queue)?)?;
        }
    }
    Ok(())
}

fn detach_queue_entries(
    queue: &mut QueueSnapshot,
    owner: &str,
    record: &OfflineRecord,
) -> Result<bool> {
    let mut changed = false;
    for entry in &mut queue.items {
        if entry.resource["download_task_id"].as_str() == Some(record.task_id.as_str())
            && entry.resource["path"].as_str().map(std::path::Path::new)
                == Some(record.path.as_path())
        {
            let resource = entry
                .resource
                .as_object_mut()
                .ok_or(Error::State("invalid local resource"))?;
            resource.remove("download_task_id");
            resource.insert("download_ledger".into(), serde_json::to_value(record)?);
            resource.insert("download_ledger_owner".into(), json!(owner));
            changed = true;
        }
    }
    Ok(changed)
}
impl OfflineLibrary {
    fn detached_access(
        &self,
        token: &SessionToken,
        record: &OfflineRecord,
    ) -> Result<OfflineAccess> {
        self.session.with_token(token, |_| Ok(()))?;
        if record.encrypted {
            offline_access(
                Some(&record.privilege),
                record.quality,
                &self.membership.snapshot()?.state,
                OfflineSource::Downloaded { complete: true },
                true,
            )
        } else {
            Ok(OfflineAccess {
                can_play: true,
                reason: None,
            })
        }
    }
    pub(crate) async fn check_detached(
        &self,
        token: &SessionToken,
        record: OfflineRecord,
    ) -> Result<OfflineCheck> {
        let access = self.detached_access(token, &record)?;
        if !access.can_play {
            return Ok(OfflineCheck {
                access,
                record,
                integrity_verified: false,
            });
        }
        let input = record.clone();
        tokio::task::spawn_blocking(move || super::service::verify(&input))
            .await
            .map_err(|_| Error::State("offline integrity worker stopped"))??;
        let access = self.detached_access(token, &record)?;
        self.session.with_token(token, |_| Ok(()))?;
        Ok(OfflineCheck {
            access,
            record,
            integrity_verified: true,
        })
    }
}
