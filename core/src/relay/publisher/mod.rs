//! Durable outbound M/401 contracts. Native queue recording and network sending
//! are separate: no account/device lock is held by the recorder across HTTP.
pub mod player;
mod record;
mod recovery;
mod send;
mod types;
use super::{RelayApi, RelayConfig, RelayService, RelaySettings, RelayUpload};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{StateStore, StateTransaction},
};
pub use recovery::RecoveryReceipt;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
pub use types::*;
const STATE: &str = "relay-publication-v1";
const JOURNAL: &str = "relay-publication-intents-v1";
const MAX_PREPARED: usize = 256;
pub struct RelayPublisher {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    config: Mutex<Option<(SessionToken, String, RelayConfig)>>,
}
fn now() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64
}
fn key(device: &str) -> String {
    hex::encode(Sha256::digest(device.as_bytes()))
}
fn active(device: &str) -> String {
    format!("relay-publication-active-{}", key(device))
}
fn load(tx: &StateTransaction<'_>, owner: &str, device: &str) -> Result<PublicationState> {
    let state: PublicationState = tx
        .get(owner, STATE, &key(device))?
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_else(|| PublicationState {
            device_id: device.into(),
            ..Default::default()
        });
    if state.device_id != device {
        return Err(Error::State("relay publisher device mismatch"));
    }
    if let Some(machine) = &state.machine {
        machine.validate()?;
    }
    if let Some(frame) = &state.frame {
        frame.validate()?;
    }
    Ok(state)
}
fn save(tx: &mut StateTransaction<'_>, owner: &str, state: &PublicationState) -> Result<()> {
    // Network/recording paths inspect only a bounded working set. Historical
    // receipts remain addressable by UUID without scanning every prior upload.
    for row in records(tx, owner, &state.device_id)? {
        if !matches!(
            row.phase,
            PublicationPhase::Prepared | PublicationPhase::Sending
        ) && state.blocked.as_deref() != Some(row.id.as_str())
            && state.recovery.as_deref() != Some(row.id.as_str())
        {
            tx.delete(owner, &active(&state.device_id), &row.id)?;
        }
    }
    tx.put(
        owner,
        STATE,
        &key(&state.device_id),
        &serde_json::to_value(state)?,
    )
}
fn records(tx: &StateTransaction<'_>, owner: &str, device: &str) -> Result<Vec<PublicationIntent>> {
    rows_in(tx, owner, device, &active(device))
}
fn rows_in(
    tx: &StateTransaction<'_>,
    owner: &str,
    device: &str,
    namespace: &str,
) -> Result<Vec<PublicationIntent>> {
    let mut rows = tx
        .values(owner, namespace)?
        .into_iter()
        .map(|(id, raw)| {
            super::validate_id(&id)?;
            let row: PublicationIntent = serde_json::from_value(raw)?;
            if row.id != id {
                return Err(Error::State("relay publisher intent identity mismatch"));
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    rows.retain(|row| row.device_id == device);
    rows.sort_by_key(|row| row.sequence);
    Ok(rows)
}
fn put(tx: &mut StateTransaction<'_>, owner: &str, row: &PublicationIntent) -> Result<()> {
    let raw = serde_json::to_value(row)?;
    tx.put(owner, JOURNAL, &row.id, &raw)?;
    tx.put(owner, &active(&row.device_id), &row.id, &raw)
}
fn append(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    state: &mut PublicationState,
    upload: RelayUpload,
    recovery_of: Option<String>,
) -> Result<PublicationIntent> {
    append_named(
        tx,
        owner,
        state,
        upload,
        recovery_of,
        uuid::Uuid::new_v4().to_string(),
    )
}
fn append_named(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    state: &mut PublicationState,
    upload: RelayUpload,
    recovery_of: Option<String>,
    id: String,
) -> Result<PublicationIntent> {
    state.sequence = state
        .sequence
        .checked_add(1)
        .ok_or(Error::State("relay publication sequence exhausted"))?;
    let row = PublicationIntent {
        id,
        sequence: state.sequence,
        phase: PublicationPhase::Prepared,
        upload,
        recovery_of,
        frame_revision: state.frame.as_ref().map_or(0, |f| f.queue_revision),
        service_code: None,
        created_at: now(),
        device_id: state.device_id.clone(),
    };
    state.latest = Some(row.id.clone());
    put(tx, owner, &row)?;
    Ok(row)
}
impl RelayPublisher {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            config: Mutex::new(None),
        })
    }
    pub(crate) fn live_config(&self, token: &SessionToken) -> Result<Option<RelayConfig>> {
        let (_, device) = self.identity(token)?;
        Ok(self
            .config
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|(t, d, _)| t == token && d == &device)
            .map(|(_, _, c)| c.clone()))
    }
    fn identity(&self, token: &SessionToken) -> Result<(String, String)> {
        let api = self.session.native_api(token)?;
        let device = api
            .local_device_id()?
            .ok_or(Error::State("native relay device identity unavailable"))?;
        super::types::validate_device(&device)?;
        self.session.require_api(token, &api)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        Ok((owner, device))
    }
    fn check_identity(&self, token: &SessionToken, owner: &str, device: &str) -> Result<()> {
        let current = self.identity(token)?;
        if current.0 != owner || current.1 != device {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    pub fn status_for(&self, token: &SessionToken) -> Result<PublicationStatus> {
        let (owner, device) = self.identity(token)?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let state = load(tx, &owner, &device)?;
                let rows = records(tx, &owner, &device)?;
                Ok(PublicationStatus {
                    pending: rows
                        .iter()
                        .filter(|r| {
                            matches!(
                                r.phase,
                                PublicationPhase::Prepared | PublicationPhase::Sending
                            )
                        })
                        .count(),
                    blocked: state
                        .blocked
                        .as_ref()
                        .map(|id| {
                            rows.iter()
                                .find(|r| &r.id == id)
                                .cloned()
                                .ok_or(Error::State("relay publication blocked receipt missing"))
                        })
                        .transpose()?,
                    latest: state
                        .latest
                        .as_ref()
                        .map(|id| {
                            tx.get(&owner, JOURNAL, id)?
                                .ok_or(Error::State("relay publication latest receipt missing"))
                                .and_then(|raw| serde_json::from_value(raw).map_err(Into::into))
                        })
                        .transpose()?,
                    recovering: state
                        .recovery
                        .as_ref()
                        .map(|id| {
                            rows.iter()
                                .find(|r| &r.id == id)
                                .cloned()
                                .ok_or(Error::State("relay publication recovery receipt missing"))
                        })
                        .transpose()?,
                    can_resynchronize: state.recovery.is_none()
                        && state.blocked.is_some()
                        && state
                            .frame
                            .as_ref()
                            .is_some_and(PublicationFrame::can_publish),
                })
            })
        })
    }
    pub fn intents_for(&self, token: &SessionToken) -> Result<Vec<PublicationIntent>> {
        let (owner, device) = self.identity(token)?;
        self.session.with_token(token, |_| {
            self.store
                .transaction(|tx| rows_in(tx, &owner, &device, JOURNAL))
        })
    }
    fn recover(&self, owner: &str, device: &str) -> Result<()> {
        self.store.transaction(|tx| {
            let mut state = load(tx, owner, device)?;
            let mut changed = false;
            for mut row in records(tx, owner, device)? {
                if row.phase == PublicationPhase::Sending {
                    changed = true;
                    row.phase = PublicationPhase::Uncertain;
                    state.blocked = Some(row.id.clone());
                    state.recovery = None;
                    put(tx, owner, &row)?;
                }
            }
            if changed {
                save(tx, owner, &state)
            } else {
                Ok(())
            }
        })
    }
}
