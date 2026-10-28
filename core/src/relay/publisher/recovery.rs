use super::*;
use serde::{Deserialize, Serialize};

const CANCELLED: &str = "relay-publication-cancelled-v1";
#[derive(Serialize, Deserialize)]
struct Cancellation {
    id: String,
    blocked: String,
    device: String,
    created_at: u64,
}
pub enum RecoveryReceipt {
    Recorded(PublicationReceipt),
    Cancelled {
        id: String,
        blocked: String,
        created_at: u64,
    },
}
impl RelayPublisher {
    /// A read may recover an abandoned Sending record only while owning the
    /// same file lock as the sender. It never dispatches or replays a request.
    pub fn inspect_for(&self, token: &SessionToken) -> Result<PublicationStatus> {
        let relay = RelayService::new(self.session.clone())?;
        match relay.lock(token) {
            Ok((owner, _lock)) => {
                let (_, device) = self.identity(token)?;
                self.recover(&owner, &device)?;
            }
            Err(Error::MutationBusy) => (),
            Err(error) => return Err(error),
        }
        self.compact_for(token)?;
        self.status_for(token)
    }
    /// One local maintenance pass. Legacy body scanning is keyset-paged; newer
    /// writes are indexed directly. Does not send, cancel or retry any work.
    pub fn compact_for(&self, token: &SessionToken) -> Result<()> {
        let (owner, device) = self.identity(token)?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let state = load(tx, &owner, &device)?;
                retention::maintain(tx, &owner, &state)
            })
        })
    }
    pub fn recovery_receipt_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<RecoveryReceipt>> {
        super::super::validate_id(id)?;
        let (owner, device) = self.identity(token)?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let receipt = if let Some(raw) = tx.get(&owner, JOURNAL, id)? {
                    Some(PublicationReceipt::from(serde_json::from_value::<
                        PublicationIntent,
                    >(raw)?))
                } else {
                    let receipt: Option<PublicationReceipt> = tx
                        .get(&owner, retention::RECEIPTS, id)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    if receipt.as_ref().is_some_and(|r| {
                        matches!(
                            r.phase,
                            PublicationPhase::Prepared | PublicationPhase::Sending
                        )
                    }) {
                        return Err(Error::State("active relay intent cannot be compacted"));
                    }
                    receipt
                };
                if let Some(row) = receipt {
                    if row.id != id || row.device_id != device {
                        return Err(Error::InvalidInput("relay recovery identity mismatch"));
                    }
                    return Ok(Some(RecoveryReceipt::Recorded(row)));
                }
                tx.get(&owner, CANCELLED, id)?
                    .map(|raw| {
                        let row: Cancellation = serde_json::from_value(raw)?;
                        if row.id != id || row.device != device {
                            return Err(Error::InvalidInput("relay recovery identity mismatch"));
                        }
                        Ok(RecoveryReceipt::Cancelled {
                            id: row.id,
                            blocked: row.blocked,
                            created_at: row.created_at,
                        })
                    })
                    .transpose()
            })
        })
    }
    /// Local-only cancellation fence for an IPC request that may arrive late.
    /// Existing durable writes retain their real outcome; nothing is resent.
    pub fn cancel_unseen_for(
        &self,
        token: &SessionToken,
        id: &str,
        blocked: &str,
    ) -> Result<RecoveryReceipt> {
        super::super::validate_id(id)?;
        super::super::validate_id(blocked)?;
        let relay = RelayService::new(self.session.clone())?;
        let (owner, _lock) = relay.lock(token)?;
        let (_, device) = self.identity(token)?;
        if let Some(receipt) = self.recovery_receipt_for(token, id)? {
            let original = match &receipt {
                RecoveryReceipt::Recorded(row) => row.recovery_of.as_deref(),
                RecoveryReceipt::Cancelled { blocked, .. } => Some(blocked.as_str()),
            };
            if original != Some(blocked) {
                return Err(Error::InvalidInput("relay recovery intent reused"));
            }
            return Ok(receipt);
        }
        let created_at = now();
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let state = load(tx, &owner, &device)?;
                if state.blocked.as_deref() != Some(blocked) {
                    return Err(Error::StaleResponse);
                }
                tx.put(
                    &owner,
                    CANCELLED,
                    id,
                    &serde_json::to_value(Cancellation {
                        id: id.into(),
                        blocked: blocked.into(),
                        device,
                        created_at,
                    })?,
                )
            })
        })?;
        Ok(RecoveryReceipt::Cancelled {
            id: id.into(),
            blocked: blocked.into(),
            created_at,
        })
    }
}
