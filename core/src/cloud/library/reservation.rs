//! Local removal reserves the remote intent first. Only Preparing may send;
//! interrupted Sending is reconciled by reads and never transmitted again.
use super::*;

impl CloudLibrary {
    pub(crate) async fn reserve_delete(
        &self,
        id: &str,
        ids: &[String],
        revision: &str,
    ) -> Result<DeleteOperation> {
        types::valid_id(id)?;
        let ids = types::checked_ids(ids)?;
        let _lock = self.lock()?;
        if let Some(op) = self.find_operation(id)? {
            if op.ids != ids || op.before_revision != revision {
                return Err(Error::State("cloud deletion intent changed"));
            }
            return Ok(op);
        }
        if self
            .operations()?
            .iter()
            .any(|op| op.phase.pending() && op.ids.iter().any(|id| ids.contains(id)))
        {
            return Err(Error::MutationUncertain);
        }
        let before = self.fetch().await?;
        if before.revision != revision {
            return Err(Error::QueueChanged);
        }
        if ids
            .iter()
            .any(|id| !before.items.iter().any(|row| &row.song_id == id))
        {
            return Err(Error::InvalidInput(
                "cloud selection is not owned by account",
            ));
        }
        let op = DeleteOperation {
            id: id.into(),
            created_ms: types::now_ms()?,
            phase: DeletePhase::Preparing,
            ids: ids.clone(),
            removed: vec![],
            remaining: ids.clone(),
            before_revision: revision.into(),
            after_revision: None,
            batches: ids
                .chunks(1000)
                .map(|ids| DeleteBatch {
                    ids: ids.into(),
                    phase: BatchPhase::Queued,
                    succeeded: vec![],
                    failed: vec![],
                    code: None,
                })
                .collect(),
        };
        self.save(&op)?;
        Ok(op)
    }

    pub(crate) async fn send_reserved_delete(&self, id: &str) -> Result<DeleteOperation> {
        let _lock = self.lock()?;
        let mut op = self.operation(id)?;
        if op.phase != DeletePhase::Preparing {
            return Ok(op);
        }
        let current = self.fetch().await?;
        if current.revision != op.before_revision {
            let present = current
                .items
                .iter()
                .map(|r| &r.song_id)
                .collect::<BTreeSet<_>>();
            op.removed = op
                .ids
                .iter()
                .filter(|id| !present.contains(id))
                .cloned()
                .collect();
            op.remaining = op
                .ids
                .iter()
                .filter(|id| present.contains(id))
                .cloned()
                .collect();
            op.after_revision = Some(current.revision);
            op.phase = DeletePhase::NotSubmitted;
            self.save(&op)?;
            return Err(Error::QueueChanged);
        }
        op.phase = DeletePhase::Sending;
        self.save(&op)?;
        self.transmit(op).await
    }

    pub(crate) fn find_operation(&self, id: &str) -> Result<Option<DeleteOperation>> {
        types::valid_id(id)?;
        let value = self.session.with_token(&self.token, |_| {
            self.store.get(&self.owner, service::OPERATIONS, id)
        })?;
        value
            .map(|value| {
                let op: DeleteOperation = serde_json::from_value(value)?;
                op.validate()?;
                if op.id != id {
                    return Err(Error::State("cloud deletion identity mismatch"));
                }
                Ok(op)
            })
            .transpose()
    }
}
