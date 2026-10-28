//! Explicit local resolution of an uncertain song-like intent. This never
//! sends a membership write or claims the original request failed remotely.
use super::{
    AccountResources, MutationPhase, MutationRecord, ResourceKind, record,
    service::{JOURNAL, now},
    snapshot,
};
use crate::{
    Error, Result, account::SessionToken, protocol::api::identifier, storage::OperationLock,
};
use serde_json::json;

impl AccountResources {
    pub async fn recover_like_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        accept_observed: Option<bool>,
    ) -> Result<MutationRecord> {
        if revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid like recovery revision"));
        }
        self.session.with_token(token, |_| Ok(()))?;
        let id = identifier(&json!(id))?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing account"))?;
        let key = record::key(ResourceKind::Liked, &id);
        let _lock = OperationLock::acquire(self.store.directory(), uid, &key)?;
        let mut operation = self
            .load(uid, &key)?
            .ok_or(Error::State("no like intent to recover"))?;
        if operation.revision()? != revision {
            return Err(Error::State("like recovery changed"));
        }
        if !operation.phase.pending() {
            return Ok(operation);
        }
        let Some(expected) = accept_observed else {
            // A negative observation is a valid recovery view, still pending.
            return match self.reconcile_for(token, &key, operation).await {
                Ok(_) | Err(Error::MutationUncertain) => self
                    .session
                    .with_token(token, |uid| self.load(uid, &key))?
                    .ok_or(Error::State("like intent disappeared")),
                Err(error) => Err(error),
            };
        };
        // This branch is only reached by explicit acceptance after a completed
        // read. A stale dialog must never dismiss a newer observation/intent.
        if operation.phase != MutationPhase::Uncertain
            || operation.checked_at.is_none()
            || operation.observed != Some(json!(expected))
            || operation.desired == json!(expected)
        {
            return Err(Error::State("reconcile like state before accepting it"));
        }
        let original = serde_json::to_value(&operation)?;
        for _ in 0..2 {
            let current = self.refresh_for(token, ResourceKind::Liked).await?;
            if snapshot::value(ResourceKind::Liked, &id, &current)? != json!(expected) {
                return Err(Error::State("like recovery changed"));
            }
        }
        operation.phase = MutationPhase::AcceptedCurrentState;
        operation.checked_at = Some(now()?);
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                let current: MutationRecord = serde_json::from_value(
                    tx.get(uid, JOURNAL, &key)?
                        .ok_or(Error::State("like intent disappeared"))?,
                )?;
                if current.revision()? != revision {
                    return Err(Error::State("like recovery changed"));
                }
                tx.put(
                    uid,
                    "resource-recovery-history-v1",
                    revision,
                    &json!({"original":original,"resolution":operation}),
                )?;
                tx.put(uid, JOURNAL, &key, &serde_json::to_value(&operation)?)
            })
        })?;
        Ok(operation)
    }
}
