use super::*;
impl CloudLibrary {
    pub async fn delete(&self, ids: &[String], revision: &str) -> Result<DeleteOperation> {
        let ids = types::checked_ids(ids)?;
        let _lock = self.lock()?;
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
        let mut op = DeleteOperation {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_ms: types::now_ms()?,
            phase: DeletePhase::Sending,
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
        for index in 0..op.batches.len() {
            op.batches[index].phase = BatchPhase::Sending;
            self.save(&op)?;
            let ids = op.batches[index]
                .ids
                .iter()
                .map(|id| json!(id))
                .collect::<Vec<_>>();
            let result = self
                .session
                .run_for(&self.token, |api, _| async move {
                    CloudApi::new(api).delete(&ids).await
                })
                .await;
            self.guard()?;
            match result {
                Ok(response) => {
                    if let Ok((succeeded, failed)) = ack(&response, &op.batches[index].ids) {
                        let batch = &mut op.batches[index];
                        batch.phase = BatchPhase::Acknowledged;
                        batch.succeeded = succeeded;
                        batch.failed = failed;
                        self.save(&op)?;
                    } else {
                        break;
                    }
                }
                Err(Error::Service { code, .. }) => {
                    let batch = &mut op.batches[index];
                    batch.phase = BatchPhase::Rejected;
                    batch.code = code.as_i64();
                    batch.failed = batch.ids.clone();
                    self.save(&op)?;
                }
                Err(Error::StaleResponse | Error::Storage(_)) => return result.map(|_| op),
                Err(_) => break,
            }
        }
        op.phase = DeletePhase::Verifying;
        self.save(&op)?;
        // Reads may fail, but a recorded write outcome must remain reviewable.
        match self.observe(&mut op).await {
            Ok(()) => {}
            Err(e @ (Error::StaleResponse | Error::Storage(_))) => return Err(e),
            Err(_) => {
                op.phase = DeletePhase::Uncertain;
                self.save(&op)?;
            }
        }
        Ok(op)
    }
    pub async fn reconcile_delete(&self, id: &str) -> Result<DeleteOperation> {
        let _lock = self.lock()?;
        let mut op = self.operation(id)?;
        if !op.phase.pending() {
            return Ok(op);
        }
        self.observe(&mut op).await?;
        Ok(op)
    }
    async fn observe(&self, op: &mut DeleteOperation) -> Result<()> {
        let current = self.fetch().await?;
        let present = current
            .items
            .iter()
            .map(|row| &row.song_id)
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
        let unresolved = op.batches.iter().any(|batch| match batch.phase {
            BatchPhase::Sending => batch.ids.iter().any(|id| present.contains(id)),
            BatchPhase::Acknowledged => batch.succeeded.iter().any(|id| present.contains(id)),
            _ => false,
        });
        op.phase = if op.remaining.is_empty() {
            DeletePhase::Confirmed
        } else if unresolved {
            DeletePhase::Uncertain
        } else if op.removed.is_empty() {
            DeletePhase::Rejected
        } else {
            DeletePhase::Partial
        };
        op.after_revision = Some(current.revision);
        self.save(op)
    }
}
fn ack(response: &Value, ids: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let get = |name| -> Result<Vec<String>> {
        response[name]
            .as_array()
            .ok_or(Error::Response("missing cloud deletion outcomes"))?
            .iter()
            .map(crate::protocol::api::identifier)
            .collect()
    };
    let success = get("succIds")?;
    let failed = get("failIds")?;
    let mut seen = BTreeSet::new();
    if success
        .iter()
        .chain(&failed)
        .any(|id| !ids.contains(id) || !seen.insert(id))
        || seen.len() != ids.len()
    {
        return Err(Error::Response(
            "cloud deletion response does not cover requested IDs",
        ));
    }
    Ok((success, failed))
}
