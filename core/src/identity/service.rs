use super::*;
use crate::{Error, Result, protocol::http::ClientProfile, storage::StateStore};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
const OWNER: &str = "device";
const KIND: &str = "identity";
const KEY: &str = "client_sign";

/// Private, installation-scoped facts, separate from an account's deviceId.
/// No Debug/Serialize: use summary() for diagnostics and profile() for transport.
pub struct DeviceIdentity {
    parts: [String; 3],
    persistent_id: String,
    summary: IdentitySummary,
}
impl DeviceIdentity {
    pub fn persistent_id(&self) -> &str {
        &self.persistent_id
    }
    pub fn summary(&self) -> &IdentitySummary {
        &self.summary
    }
    pub fn client_sign(&self) -> Result<String> {
        client_sign(
            [
                &self.parts[0],
                &self.parts[1],
                &self.parts[2],
                &self.persistent_id,
            ],
            15,
        )
    }
    pub fn profile(&self, mut base: ClientProfile) -> Result<ClientProfile> {
        base.client_sign = Some(self.client_sign()?);
        base.validate()?;
        Ok(base)
    }
}
pub struct IdentityService {
    store: Arc<StateStore>,
}
fn stored_id(value: &Value) -> Result<&str> {
    value["persistent_id"]
        .as_str()
        .filter(|v| valid_persistent_id(v))
        .ok_or(Error::State(
            "stored persistent identity is invalid; rotation refused",
        ))
}
impl IdentityService {
    pub fn new(store: Arc<StateStore>) -> Self {
        Self { store }
    }
    /// BEGIN IMMEDIATE atomically creates or adopts the persisted ID and updates
    /// observations; independent connections cannot choose competing first IDs.
    pub fn resolve(&self, facts: &HardwareFacts) -> Result<DeviceIdentity> {
        let persistent_id=self.store.transaction(|tx| {
            let id=match tx.get(OWNER,KIND,KEY)? {
                Some(value)=>stored_id(&value)?.to_owned(),
                None=>{
                    let id=facts.derived_persistent_id();
                    tx.put(OWNER,KIND,KEY,&json!({"persistent_id":id,"creation_source":facts.source}))?;
                    id
                }
            };
            tx.put(OWNER,KIND,"last_observed",&json!({"parts":[facts.parts[0],facts.parts[1],facts.parts[2],id],"source":facts.source}))?;
            Ok(id)
        })?;
        Ok(DeviceIdentity {
            parts: facts.parts.clone(),
            persistent_id,
            summary: facts.summary(),
        })
    }
    /// Explicit primary-record import only. The unrecovered secondary hardware
    /// key is never guessed, and a different existing ID is never overwritten.
    pub fn import_primary(&self, bytes: &[u8]) -> Result<()> {
        let records = parse_container(bytes)?;
        let id = decode_record(
            records
                .get(&ID_RECORD_KEY)
                .ok_or(Error::InvalidInput("device ID record missing"))?,
        )?;
        let id = std::str::from_utf8(&id)
            .ok()
            .filter(|v| valid_persistent_id(v))
            .ok_or(Error::InvalidInput(
                "device ID record is not 64 lowercase hexadecimal characters",
            ))?;
        if let Some(marker) = records.get(&MARKER_RECORD_KEY) {
            // Require intact record encoding, not a particular marker value.
            decode_record(marker)?;
        }
        self.store.transaction(|tx| {
            if let Some(value)=tx.get(OWNER,KIND,KEY)? {
                if stored_id(&value)? != id {return Err(Error::State("device identity import would rotate existing ID"));}
                return Ok(());
            }
            tx.put(OWNER,KIND,KEY,&json!({"persistent_id":id,"creation_source":{"platform":"explicit-primary-container-import"}}))
        })
    }
    /// Export primary ID and fixed marker only, no secondary hardware record.
    /// Does not touch official paths or modify credential records.
    pub fn export_primary(&self) -> Result<Vec<u8>> {
        let record = self
            .store
            .get(OWNER, KIND, KEY)?
            .ok_or(Error::State("device identity is not initialized"))?;
        let id = stored_id(&record)?;
        encode_container(&BTreeMap::from([
            (ID_RECORD_KEY, encode_record(id.as_bytes())?),
            (MARKER_RECORD_KEY, encode_record(MARKER)?),
        ]))
    }
}
