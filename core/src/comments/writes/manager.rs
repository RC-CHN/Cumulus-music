use super::{model::validate_id, *};
use crate::{
    Error, Result,
    account::SessionToken,
    comments::{CommentApi, Comments},
    storage::OperationLock,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
impl Comments {
    pub(super) fn write_lock(
        &self,
        token: &SessionToken,
        resource: &crate::comments::CommentResource,
    ) -> Result<OperationLock> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        OperationLock::acquire(
            self.store.directory(),
            "comment-write",
            &serde_json::to_string(&(owner, resource.thread()?))?,
        )
    }
    pub(super) fn read_write(&self, owner: &str, id: &str) -> Result<CommentWrite> {
        validate_id(id)?;
        let record: CommentWrite = serde_json::from_value(
            self.store
                .get(owner, JOURNAL, id)?
                .ok_or(Error::State("comment write not found"))?,
        )?;
        record.validate()?;
        if record.id != id {
            return Err(Error::State("comment write journal identity mismatch"));
        }
        Ok(record)
    }
    pub fn write_operations_for(
        &self,
        token: &SessionToken,
        resource: &crate::comments::CommentResource,
    ) -> Result<Vec<CommentWrite>> {
        resource.thread()?;
        self.session.with_token(token, |owner| {
            let mut records = Vec::new();
            for id in self.store.values(owner, JOURNAL)?.into_keys() {
                let record = self.read_write(owner, &id)?;
                if &record.resource == resource {
                    records.push(record);
                }
            }
            records.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
            Ok(records)
        })
    }
    pub fn write_operation_for(&self, token: &SessionToken, id: &str) -> Result<CommentWrite> {
        self.session
            .with_token(token, |owner| self.read_write(owner, id))
    }
    pub async fn prepare_write_for(
        &self,
        token: &SessionToken,
        resource: &crate::comments::CommentResource,
        command: CommentWriteCommand,
    ) -> Result<CommentWrite> {
        command.validate()?;
        let _lock = self.write_lock(token, resource)?;
        if self
            .write_operations_for(token, resource)?
            .iter()
            .any(|v| v.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let before = if let Some(id) = command.target() {
            Some(self.target_for(token, resource, id).await?)
        } else {
            None
        };
        self.check_write_target(token, &command, before.as_ref())?;
        let record = CommentWrite {
            id: uuid::Uuid::new_v4().simple().to_string(),
            resource: resource.clone(),
            command,
            phase: CommentWritePhase::Prepared,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("invalid system time"))?
                .as_millis()
                .try_into()
                .map_err(|_| Error::State("time exceeds comment bounds"))?,
            acknowledged: false,
            receipt_id: None,
            receipt: None,
            before,
            observed: None,
            observed_absent: false,
            service_code: None,
        };
        self.save_write_transition(token, &record)?;
        Ok(record)
    }
    fn check_write_target(
        &self,
        token: &SessionToken,
        command: &CommentWriteCommand,
        target: Option<&crate::comments::Comment>,
    ) -> Result<()> {
        self.session.with_token(token, |owner| {
            if let Some(target) = target {
                if target.status.is_some_and(|s| s < 0) {
                    return Err(Error::State("comment target is unavailable"));
                }
                if matches!(command, CommentWriteCommand::Delete { .. })
                    && target.author.as_ref().is_none_or(|a| a.id != owner)
                {
                    return Err(Error::InvalidInput("only own comments may be deleted"));
                }
            }
            Ok(())
        })
    }
    pub async fn submit_write_for(&self, token: &SessionToken, id: &str) -> Result<CommentWrite> {
        let mut record = self.write_operation_for(token, id)?;
        let _lock = self.write_lock(token, &record.resource)?;
        record = self.write_operation_for(token, id)?;
        if record.phase != CommentWritePhase::Prepared {
            return if record.phase.pending() {
                self.settle_write(token, record).await
            } else {
                Ok(record)
            };
        }
        // Revalidate the target immediately before the one permitted send.
        if let Some(target) = record.command.target() {
            let observed = self.target_for(token, &record.resource, target).await?;
            self.check_write_target(token, &record.command, Some(&observed))?;
        }
        record.phase = CommentWritePhase::Sending;
        self.save_write_transition(token, &record)?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("comment owner missing"))?;
        let thread = record.resource.thread()?;
        self.session
            .run_for(token, |api, _| async {
                let result = CommentApi::new(api)
                    .write(&record.resource, &record.command)
                    .await;
                match &result {
                    Ok(value) => {
                        record.acknowledged = true;
                        record.service_code = Some(200);
                        record.phase = CommentWritePhase::Verifying;
                        if matches!(record.command, CommentWriteCommand::Publish { .. }) {
                            let reply = &value["comment"];
                            let same_thread = reply
                                .get("threadId")
                                .is_none_or(|v| v.as_str() == Some(thread.as_str()));
                            record.receipt_id = if same_thread {
                                crate::protocol::api::identifier(&reply["commentId"]).ok()
                            } else {
                                None
                            };
                            record.receipt = record
                                .receipt_id
                                .as_ref()
                                .and_then(|_| crate::comments::Comment::from_official(reply).ok());
                            if record.receipt_id.is_none() {
                                record.phase = CommentWritePhase::Uncertain;
                            }
                        }
                    }
                    Err(error) => {
                        record.service_code = [250, 301, 403, 404, 405, 480, 505, 506, 511, 512]
                            .into_iter()
                            .find(|code| error.is_service_code(*code));
                        record.phase = if record.service_code.is_some() {
                            CommentWritePhase::Rejected
                        } else {
                            CommentWritePhase::Uncertain
                        };
                    }
                }
                // Preserve even a late receipt under its original owner before the
                // AccountSession rejects delivery into another generation.
                self.store
                    .put(owner, JOURNAL, id, &serde_json::to_value(&record)?)?;
                result.map(|_| ())
            })
            .await?;
        self.settle_write(token, record).await
    }
    /// Explicitly stop tracking, never an assertion that a public write failed
    /// or was undone. Keeps all receipt/unknown-result facts in the journal.
    pub fn abandon_write_for(&self, token: &SessionToken, id: &str) -> Result<CommentWrite> {
        let record = self.write_operation_for(token, id)?;
        let _lock = self.write_lock(token, &record.resource)?;
        let mut record = self.write_operation_for(token, id)?;
        if record.phase.pending() {
            record.phase = CommentWritePhase::Abandoned;
            self.save_write_transition(token, &record)?;
        }
        Ok(record)
    }
    pub(super) fn save_write_transition(
        &self,
        token: &SessionToken,
        record: &CommentWrite,
    ) -> Result<()> {
        record.validate()?;
        let thread = record.resource.thread()?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(owner, JOURNAL, &record.id, &serde_json::to_value(record)?)?;
                tx.put(
                    owner,
                    crate::comments::cache::REVISIONS,
                    &thread,
                    &json!(uuid::Uuid::new_v4().simple().to_string()),
                )?;
                // Posting/deletion changes pagination and quoted references. Evict
                // the exact resource, not an optimistic guessed total/order.
                for kind in [
                    crate::comments::cache::PAGES,
                    crate::comments::floor::FLOORS,
                ] {
                    for row in tx.rows(kind)? {
                        if row.owner == owner
                            && (row.value.get("resource").is_none()
                                || row.value["resource"] == serde_json::to_value(&record.resource)?)
                        {
                            tx.delete(owner, kind, &row.key)?;
                        }
                    }
                }
                Ok(())
            })
        })
    }
}
