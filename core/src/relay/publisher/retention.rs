//! Community persistence policy, not an additional official API contract.
//! Full old uploads are expensive; minimal UUID outcomes are deliberately kept
//! indefinitely, including cancellation fences, to prevent late IPC replay.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const RECEIPTS: &str = "relay-publication-receipts-v1";
const MIGRATION: &str = "relay-publication-retention-v1";
const PAGE: usize = 32;
const MAX_RECENT: usize = 128;
const MAX_RECENT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Migration {
    cursor: Option<String>,
    complete: bool,
}
#[derive(Serialize, Deserialize)]
struct Indexed {
    id: String,
    sequence: u64,
    bytes: usize,
}
fn namespace(device: &str) -> String {
    format!("relay-publication-order-{}", key(device))
}
fn index_key(sequence: u64, id: &str) -> String {
    format!("{sequence:020}-{id}")
}
pub(super) fn index(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    row: &PublicationIntent,
    raw: &Value,
) -> Result<()> {
    index_size(tx, owner, row, serde_json::to_vec(raw)?.len())
}
fn index_size(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    row: &PublicationIntent,
    bytes: usize,
) -> Result<()> {
    tx.put(
        owner,
        &namespace(&row.device_id),
        &index_key(row.sequence, &row.id),
        &serde_json::to_value(Indexed {
            id: row.id.clone(),
            sequence: row.sequence,
            bytes,
        })?,
    )
}
fn migrate(tx: &mut StateTransaction<'_>, owner: &str, device: &str) -> Result<()> {
    let mut progress: Migration = tx
        .get(owner, MIGRATION, &key(device))?
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    if progress.complete {
        return Ok(());
    }
    let rows = tx.values_page(owner, JOURNAL, progress.cursor.as_deref(), PAGE)?;
    let count = rows.len();
    let last = rows.keys().next_back().cloned();
    for (id, raw) in rows {
        let bytes = serde_json::to_vec(&raw)?.len();
        let row: PublicationIntent = serde_json::from_value(raw)?;
        if row.id != id {
            return Err(Error::State("relay journal identity mismatch"));
        }
        super::super::validate_id(&id)?;
        if row.device_id == device {
            index_size(tx, owner, &row, bytes)?;
        }
    }
    if let Some(last) = last {
        progress.cursor = Some(last);
    }
    progress.complete = count < PAGE;
    tx.put(
        owner,
        MIGRATION,
        &key(device),
        &serde_json::to_value(progress)?,
    )
}

pub(super) fn maintain(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    state: &PublicationState,
) -> Result<()> {
    migrate(tx, owner, &state.device_id)?;
    let namespace = namespace(&state.device_id);
    // The index contains only full bodies, not every historical UUID. Normal
    // work is bounded by recent bodies + active work; legacy migration adds at
    // most PAGE rows per transaction. Never enumerate the compact receipts.
    let mut count = 0usize;
    let mut bytes = 0usize;
    for (key, raw) in tx.values(owner, &namespace)?.into_iter().rev() {
        let indexed: Indexed = serde_json::from_value(raw)?;
        if key != index_key(indexed.sequence, &indexed.id) {
            return Err(Error::State("relay journal index mismatch"));
        }
        if [
            state.blocked.as_ref(),
            state.recovery.as_ref(),
            state.latest.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|id| id == &indexed.id)
        {
            continue;
        }
        // Active entries stay full, even if a legacy database has more of them
        // than the current backlog limit. Retention is never cancellation.
        if tx
            .get(owner, &active(&state.device_id), &indexed.id)?
            .is_some()
        {
            continue;
        }
        count = count.saturating_add(1);
        bytes = bytes.saturating_add(indexed.bytes);
        if count <= MAX_RECENT && bytes <= MAX_RECENT_BYTES {
            continue;
        }
        let row: PublicationIntent = serde_json::from_value(
            tx.get(owner, JOURNAL, &indexed.id)?
                .ok_or(Error::State("relay journal body missing"))?,
        )?;
        if row.id != indexed.id
            || row.sequence != indexed.sequence
            || row.device_id != state.device_id
        {
            return Err(Error::State("relay journal body/index mismatch"));
        }
        if matches!(
            row.phase,
            PublicationPhase::Prepared | PublicationPhase::Sending
        ) {
            continue;
        }
        let id = row.id.clone();
        tx.put(
            owner,
            RECEIPTS,
            &id,
            &serde_json::to_value(PublicationReceipt::from(row))?,
        )?;
        tx.delete(owner, JOURNAL, &id)?;
        tx.delete(owner, &namespace, &key)?;
    }
    Ok(())
}
