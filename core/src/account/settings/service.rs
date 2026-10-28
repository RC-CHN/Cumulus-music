use super::{SettingsMutation, SettingsSnapshot};
use crate::{
    Error, Result,
    account::{AccountApi, AccountSession, SessionToken},
    storage::{OperationLock, StateStore, StateTransaction},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) const NAMESPACE: &str = "account-settings-v1";
pub(super) const JOURNAL: &str = "account-settings-intents-v1";
pub(super) fn now() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64
}
pub(super) fn revision(values: &Value) -> Result<String> {
    if !values.is_object() {
        return Err(Error::Response("missing account settings"));
    }
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(values)?)))
}
pub(super) fn save_snapshot(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    snapshot: &SettingsSnapshot,
) -> Result<()> {
    tx.put(
        owner,
        NAMESPACE,
        "snapshot",
        &serde_json::to_value(snapshot)?,
    )?;
    // Keep the existing account-resource cache compatible with native callers.
    tx.put(owner, "resources", "settings", &snapshot.values)
}
pub struct AccountSettings {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
}
impl AccountSettings {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    pub(super) fn lock(&self, token: &SessionToken) -> Result<(String, OperationLock)> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let lock = OperationLock::acquire(self.store.directory(), NAMESPACE, &owner)?;
        self.migrate_legacy(token)?;
        Ok((owner, lock))
    }
    pub fn snapshot_for(&self, token: &SessionToken) -> Result<Option<SettingsSnapshot>> {
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, NAMESPACE, "snapshot")?
                .map(|raw| {
                    let snapshot: SettingsSnapshot = serde_json::from_value(raw)?;
                    if snapshot.revision != revision(&snapshot.values)? {
                        return Err(Error::State("invalid account settings snapshot"));
                    }
                    Ok(snapshot)
                })
                .transpose()
        })
    }
    pub fn mutations_for(&self, token: &SessionToken) -> Result<Vec<SettingsMutation>> {
        let (_owner, _lock) = self.lock(token)?;
        self.operations(token)
    }
    pub fn inspect_for(
        &self,
        token: &SessionToken,
    ) -> Result<(Option<SettingsSnapshot>, Vec<SettingsMutation>)> {
        let (_owner, _lock) = self.lock(token)?;
        Ok((self.snapshot_for(token)?, self.operations(token)?))
    }
    pub(super) fn operations(&self, token: &SessionToken) -> Result<Vec<SettingsMutation>> {
        self.session.with_token(token, |uid| {
            self.store
                .values(uid, JOURNAL)?
                .into_iter()
                .map(|(id, raw)| {
                    let op: SettingsMutation = serde_json::from_value(raw)?;
                    super::mutation::validate_id(&id)?;
                    op.change.validate()?;
                    if op.id != id {
                        return Err(Error::State("settings journal identity mismatch"));
                    }
                    Ok(op)
                })
                .collect()
        })
    }
    pub(super) fn operation(&self, token: &SessionToken, id: &str) -> Result<SettingsMutation> {
        self.operations(token)?
            .into_iter()
            .find(|op| op.id == id)
            .ok_or(Error::State("no settings operation to reconcile"))
    }
    pub async fn refresh_for(&self, token: &SessionToken) -> Result<SettingsSnapshot> {
        let (_owner, _lock) = self.lock(token)?;
        let snapshot = self.read_remote(token).await?;
        self.session.with_token(token, |uid| {
            self.store
                .transaction(|tx| save_snapshot(tx, uid, &snapshot))
        })?;
        Ok(snapshot)
    }
    pub(super) async fn read_remote(&self, token: &SessionToken) -> Result<SettingsSnapshot> {
        let values = self
            .session
            .run_for(token, |api, _| async move {
                AccountApi::new(api).settings().await
            })
            .await?;
        Ok(SettingsSnapshot {
            revision: revision(&values)?,
            fetched_at: now(),
            values,
        })
    }
}
