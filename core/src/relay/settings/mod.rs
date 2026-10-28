//! Device-bound RELAY_SWITCH lifecycle. Reads and writes share the relay lock;
//! the wire target is always the selected native transport's local identity.
mod mutation;
use super::{RelayApi, RelayService};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken, settings::SettingsPhase},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const NAMESPACE: &str = "relay-setting-v1";
const JOURNAL: &str = "relay-setting-intents-v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelaySettingSnapshot {
    pub revision: String,
    pub enabled: Option<bool>,
    pub fetched_at: u64,
    pub(crate) device_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelaySettingIntent {
    pub id: String,
    pub expected_revision: String,
    pub enabled: bool,
    pub before: Option<bool>,
    pub phase: SettingsPhase,
    pub created_at: u64,
    pub last_read_at: Option<u64>,
    pub read_revision: Option<String>,
    pub observed: Option<bool>,
    pub service_code: Option<i64>,
    pub(crate) device_id: String,
}
pub struct RelaySettings {
    relay: RelayService,
}
fn now() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64
}
fn key(device: &str) -> String {
    hex::encode(Sha256::digest(device.as_bytes()))
}
impl RelaySettings {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            relay: RelayService::new(session)?,
        })
    }
    fn device(&self, token: &SessionToken) -> Result<Option<String>> {
        let api = self.relay.session.native_api(token)?;
        let device = api.local_device_id()?;
        if let Some(device) = &device {
            super::types::validate_device(device)?;
        }
        self.relay.session.require_api(token, &api)?;
        Ok(device)
    }
    fn require_device(&self, token: &SessionToken) -> Result<String> {
        self.device(token)?
            .ok_or(Error::State("native relay device identity unavailable"))
    }
    pub(super) fn cache_for(
        &self,
        token: &SessionToken,
        enabled: Option<bool>,
    ) -> Result<Option<RelaySettingSnapshot>> {
        let Some(device_id) = self.device(token)? else {
            return Ok(None);
        };
        let snapshot = RelaySettingSnapshot {
            revision: hex::encode(Sha256::digest(serde_json::to_vec(&(&device_id, enabled))?)),
            device_id,
            enabled,
            fetched_at: now(),
        };
        self.relay.session.with_token(token, |uid| {
            self.relay.store.put(
                uid,
                NAMESPACE,
                &key(&snapshot.device_id),
                &serde_json::to_value(&snapshot)?,
            )
        })?;
        Ok(Some(snapshot))
    }
    pub fn snapshot_for(&self, token: &SessionToken) -> Result<Option<RelaySettingSnapshot>> {
        let Some(device) = self.device(token)? else {
            return Ok(None);
        };
        self.relay.session.with_token(token, |uid| {
            let raw = self.relay.store.get(uid, NAMESPACE, &key(&device))?;
            raw.map(|raw| {
                let value: RelaySettingSnapshot = serde_json::from_value(raw)?;
                if value.device_id != device
                    || value.revision
                        != hex::encode(Sha256::digest(serde_json::to_vec(&(
                            &device,
                            value.enabled,
                        ))?))
                {
                    return Err(Error::State("relay setting identity mismatch"));
                }
                Ok(value)
            })
            .transpose()
        })
    }
    fn all_operations(&self, token: &SessionToken) -> Result<Vec<RelaySettingIntent>> {
        self.relay.session.with_token(token, |uid| {
            self.relay
                .store
                .values(uid, JOURNAL)?
                .into_iter()
                .map(|(id, raw)| {
                    super::validate_id(&id)?;
                    let op: RelaySettingIntent = serde_json::from_value(raw)?;
                    if op.id != id {
                        return Err(Error::State("relay setting intent mismatch"));
                    }
                    super::types::validate_device(&op.device_id)?;
                    Ok(op)
                })
                .collect()
        })
    }
    pub fn operations_for(&self, token: &SessionToken) -> Result<Vec<RelaySettingIntent>> {
        let Some(device) = self.device(token)? else {
            return Ok(vec![]);
        };
        Ok(self
            .all_operations(token)?
            .into_iter()
            .filter(|op| op.device_id == device)
            .collect())
    }
    fn operation(&self, token: &SessionToken, id: &str) -> Result<RelaySettingIntent> {
        super::validate_id(id)?;
        self.operations_for(token)?
            .into_iter()
            .find(|op| op.id == id)
            .ok_or(Error::State(
                "relay setting intent not found on current device",
            ))
    }
    fn save_original(&self, owner: &str, op: &RelaySettingIntent) -> Result<()> {
        self.relay
            .store
            .put(owner, JOURNAL, &op.id, &serde_json::to_value(op)?)
    }
    async fn read(&self, token: &SessionToken) -> Result<RelaySettingSnapshot> {
        let expected = self.require_device(token)?;
        let enabled = self
            .relay
            .session
            .run_for(
                token,
                |api, _| async move { RelayApi::new(api).setting().await },
            )
            .await?;
        if self.require_device(token)? != expected {
            return Err(Error::StaleResponse);
        }
        self.cache_for(token, enabled)?
            .ok_or(Error::State("native relay identity lost"))
    }
    pub async fn refresh_for(&self, token: &SessionToken) -> Result<RelaySettingSnapshot> {
        let (_owner, _lock) = self.relay.lock(token)?;
        self.read(token).await
    }
}
