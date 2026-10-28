use super::{
    AccountSettings, SettingChange, SettingsMutation, SettingsPhase,
    service::{JOURNAL, now, save_snapshot},
};
use crate::{
    Error, Result,
    account::{AccountApi, SessionToken},
};

pub(super) fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(Error::InvalidInput("settings intent requires a UUID"));
    }
    Ok(())
}
fn validate_intent(id: &str, revision: &str, change: &SettingChange) -> Result<()> {
    validate_id(id)?;
    change.validate()?;
    if revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid settings revision"));
    }
    Ok(())
}
fn same(op: &SettingsMutation, revision: &str, change: &SettingChange) -> Result<()> {
    if op.expected_revision != revision || &op.change != change {
        return Err(Error::InvalidInput("settings intent ID reused"));
    }
    Ok(())
}
fn intent(id: &str, revision: &str, change: SettingChange) -> SettingsMutation {
    SettingsMutation {
        id: id.into(),
        change,
        expected_revision: revision.into(),
        before: None,
        phase: SettingsPhase::Pending,
        created_at: now(),
        last_read_at: None,
        read_revision: None,
        observed: None,
        service_code: None,
    }
}
impl AccountSettings {
    pub fn cancel_unseen_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        change: SettingChange,
    ) -> Result<SettingsMutation> {
        validate_intent(id, revision, &change)?;
        let (_owner, _lock) = self.lock(token)?;
        if let Some(op) = self.operations(token)?.into_iter().find(|op| op.id == id) {
            same(&op, revision, &change)?;
            return Ok(op);
        }
        let mut op = intent(id, revision, change);
        op.phase = SettingsPhase::Cancelled;
        self.session
            .save_result(token, JOURNAL, id, &serde_json::to_value(&op)?)?;
        Ok(op)
    }
    pub async fn change_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        change: SettingChange,
    ) -> Result<SettingsMutation> {
        validate_intent(id, expected_revision, &change)?;
        let (owner, _lock) = self.lock(token)?;
        let prior = self.operations(token)?;
        if let Some(op) = prior.iter().find(|op| op.id == id) {
            same(op, expected_revision, &change)?;
            return Ok(op.clone()); // Includes Pending after a crash: NEVER replay.
        }
        if prior.iter().any(|op| op.phase.unresolved()) {
            return Err(Error::MutationUncertain);
        }
        let cached = self
            .snapshot_for(token)?
            .ok_or(Error::State("read account settings first"))?;
        if cached.revision != expected_revision {
            return Err(Error::State("account settings changed"));
        }
        if change.key.value(&cached.values[change.key.key()]).is_none() {
            return Err(Error::Response("setting is absent or unsupported"));
        }
        // Other devices can change coupled fields. Check the complete fresh
        // snapshot before writing, not only the radio button the user clicked.
        let snapshot = self.read_remote(token).await?;
        self.session.with_token(token, |uid| {
            self.store
                .transaction(|tx| save_snapshot(tx, uid, &snapshot))
        })?;
        if snapshot.revision != expected_revision {
            return Err(Error::State("account settings changed"));
        }
        let before = change
            .key
            .value(&snapshot.values[change.key.key()])
            .ok_or(Error::Response("setting is absent or unsupported"))?;
        let mut op = intent(id, expected_revision, change);
        op.before = Some(before);
        if before == op.change.value {
            op.phase = SettingsPhase::Unchanged;
        }
        self.session
            .save_result(token, JOURNAL, id, &serde_json::to_value(&op)?)?;
        if op.phase == SettingsPhase::Unchanged {
            return Ok(op);
        }
        let patch = op.change.patch();
        let result = self
            .session
            .run_for(token, |api, _| async move {
                AccountApi::new(api).update_settings(&patch).await
            })
            .await;
        if let Err(error) = result {
            op.phase = if matches!(error.root(), Error::RequestNotSent(_))
                || [301, 403, 505, 506, 511, 512]
                    .iter()
                    .any(|c| error.is_service_code(*c))
            {
                SettingsPhase::Rejected
            } else {
                SettingsPhase::Uncertain
            };
            op.service_code = match error.root() {
                Error::Service { code, .. } => code.as_i64(),
                _ => None,
            };
            // Save responsibility in the ORIGINAL owner even after sign-out.
            self.store
                .put(&owner, JOURNAL, id, &serde_json::to_value(&op)?)?;
            return Err(error);
        }
        // Success acknowledgement is not proof of state. A failed read leaves
        // the durable Pending intent blocking all coupled settings.
        self.readback(token, op).await
    }
    pub async fn reconcile_for(&self, token: &SessionToken, id: &str) -> Result<SettingsMutation> {
        validate_id(id)?;
        let (_owner, _lock) = self.lock(token)?;
        let op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        self.readback(token, op).await
    }
    async fn readback(
        &self,
        token: &SessionToken,
        mut op: SettingsMutation,
    ) -> Result<SettingsMutation> {
        let snapshot = self.read_remote(token).await?;
        op.observed = op.change.key.value(&snapshot.values[op.change.key.key()]);
        op.last_read_at = Some(snapshot.fetched_at);
        op.read_revision = Some(snapshot.revision.clone());
        op.phase = if op.observed == Some(op.change.value) {
            SettingsPhase::Confirmed
        } else {
            SettingsPhase::Uncertain
        };
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                save_snapshot(tx, uid, &snapshot)?;
                tx.put(uid, JOURNAL, &op.id, &serde_json::to_value(&op)?)
            })
        })?;
        Ok(op)
    }
    pub async fn accept_current_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
    ) -> Result<SettingsMutation> {
        validate_id(id)?;
        let (_owner, _lock) = self.lock(token)?;
        let mut op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        if op.last_read_at.is_none() || op.read_revision.as_deref() != Some(expected_revision) {
            return Err(Error::State("reconcile account settings before acceptance"));
        }
        let snapshot = self.read_remote(token).await?;
        self.session.with_token(token, |uid| {
            self.store
                .transaction(|tx| save_snapshot(tx, uid, &snapshot))
        })?;
        if snapshot.revision != expected_revision {
            return Err(Error::State("account settings changed"));
        }
        op.phase = SettingsPhase::AcceptedCurrentState;
        op.last_read_at = Some(snapshot.fetched_at);
        self.session
            .save_result(token, JOURNAL, id, &serde_json::to_value(&op)?)?;
        Ok(op)
    }
}
