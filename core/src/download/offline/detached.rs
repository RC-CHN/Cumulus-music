//! Forgetting a list entry keeps its existing local queue playable, with the
//! same account, entitlement and integrity checks. This ledger stays native.
use super::{OfflineCheck, OfflineLibrary, OfflineRecord};
use crate::{
    Error, Result,
    account::SessionToken,
    membership::{OfflineAccess, OfflineSource, offline_access},
    playback::QueueSnapshot,
    storage::StateTransaction,
};
use serde_json::json;

pub(crate) fn detach_download_queue(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    record: &OfflineRecord,
) -> Result<()> {
    let Some(value) = tx.get(owner, "playback", "queue")? else {
        return Ok(());
    };
    let mut queue: QueueSnapshot = serde_json::from_value(value)?;
    queue.validate()?;
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
    if changed {
        tx.put(owner, "playback", "queue", &serde_json::to_value(queue)?)?;
    }
    Ok(())
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
