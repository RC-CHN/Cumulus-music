use super::{HistoryApi, HistoryKind, HistoryRecord, RecordOrigin, merge_records};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) const NAMESPACE: &str = "play-history-v1";
const SYNC_PRIVACY_REVISION: &str = "sync-privacy-revision";
const SYNC_CHOICE: &str = "sync-enabled";
const SYNC_DEFAULT: &str = "sync-experiment-default";
const SYNC_FIRST_OFF: &str = "sync-first-off-confirmed";
const SYNC_VIEW_REVISION: &str = "sync-view-revision";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteHistory {
    pub kind: HistoryKind,
    pub fetched_at: u64,
    pub records: Vec<HistoryRecord>,
}
#[derive(Clone, Debug, Serialize)]
pub struct HistorySnapshot {
    pub kind: HistoryKind,
    pub revision: String,
    pub sync_enabled: bool,
    pub local: Vec<HistoryRecord>,
    pub remote: Option<RemoteHistory>,
    pub entries: Vec<HistoryRecord>,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistorySyncAction {
    Read,
    Set {
        enabled: bool,
        confirmed_first_disable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySyncState {
    pub enabled: bool,
    pub first_disable_confirmed: bool,
    #[serde(serialize_with = "decimal_revision")]
    pub revision: u64,
}

fn decimal_revision<S: serde::Serializer>(
    revision: &u64,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_str(revision)
}

pub struct HistoryService {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
}
pub(super) fn local_key(kind: HistoryKind) -> String {
    format!("local:{}", kind.key())
}
pub(super) fn remote_key(kind: HistoryKind) -> String {
    format!("remote:{}", kind.key())
}
pub(super) fn local(
    tx: &StateTransaction<'_>,
    owner: &str,
    kind: HistoryKind,
) -> Result<Vec<HistoryRecord>> {
    let rows: Vec<HistoryRecord> = tx
        .get(owner, NAMESPACE, &local_key(kind))?
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    if rows.len() > kind.local_limit() || rows.iter().any(|r| r.origin != RecordOrigin::Local) {
        return Err(Error::State("invalid local history snapshot"));
    }
    for row in &rows {
        row.validate(kind)?;
    }
    Ok(rows)
}
fn stored_sync_bool(tx: &StateTransaction<'_>, owner: &str, key: &str) -> Result<Option<bool>> {
    match tx.get(owner, NAMESPACE, key)? {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(value)),
        _ => Err(Error::State("invalid recent play preference")),
    }
}

pub(super) fn enabled(tx: &StateTransaction<'_>, owner: &str) -> Result<bool> {
    if let Some(explicit) = stored_sync_bool(tx, owner, SYNC_CHOICE)? {
        return Ok(explicit);
    }
    Ok(stored_sync_bool(tx, owner, SYNC_DEFAULT)?.unwrap_or(true))
}

fn stored_sync_revision(tx: &StateTransaction<'_>, owner: &str, key: &str) -> Result<u64> {
    match tx.get(owner, NAMESPACE, key)? {
        // Existing histories need no eager migration.
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(Error::State("invalid recent play setting revision")),
    }
}

fn sync_privacy_revision(tx: &StateTransaction<'_>, owner: &str) -> Result<u64> {
    stored_sync_revision(tx, owner, SYNC_PRIVACY_REVISION)
}

fn advance_sync_revision(tx: &mut StateTransaction<'_>, owner: &str, key: &str) -> Result<u64> {
    let next = stored_sync_revision(tx, owner, key)?
        .checked_add(1)
        .ok_or(Error::State("recent play setting revision overflow"))?;
    tx.put(owner, NAMESPACE, key, &json!(next))?;
    Ok(next)
}

fn write_sync_choice(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    current: bool,
    desired: bool,
) -> Result<()> {
    // An explicit choice matching today's default must still protect it
    // against tomorrow's cohort; there is no privacy edge if it was a no-op.
    if stored_sync_bool(tx, owner, SYNC_CHOICE)? != Some(desired) {
        tx.put(owner, NAMESPACE, SYNC_CHOICE, &json!(desired))?;
    }
    if current != desired {
        advance_sync_revision(tx, owner, SYNC_PRIVACY_REVISION)?;
    }
    Ok(())
}

pub(super) fn upsert(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    kind: HistoryKind,
    record: HistoryRecord,
) -> Result<()> {
    record.validate(kind)?;
    if record.origin != RecordOrigin::Local {
        return Err(Error::InvalidInput(
            "local recorder requires a local record",
        ));
    }
    let mut rows = local(tx, owner, kind)?;
    if rows
        .iter()
        .any(|r| r.id == record.id && r.played_at > record.played_at)
    {
        return Ok(());
    }
    rows.retain(|r| r.id != record.id);
    rows.insert(0, record);
    rows.sort_by_key(|r| std::cmp::Reverse(r.played_at));
    rows.truncate(kind.local_limit());
    tx.put(
        owner,
        NAMESPACE,
        &local_key(kind),
        &serde_json::to_value(rows)?,
    )
}
pub(super) fn revision(
    kind: HistoryKind,
    sync: bool,
    local: &[HistoryRecord],
    remote: &Option<RemoteHistory>,
) -> Result<String> {
    // A playback clock checkpoint must not invalidate a user's selection or
    // deletion confirmation. Identity/order, play time, metadata and settings
    // still fence new plays and actual page changes.
    let rows = |records: &[HistoryRecord]| {
        records
            .iter()
            .map(|r| json!([r.id, r.played_at, r.resource, r.device, r.origin]))
            .collect::<Vec<_>>()
    };
    let remote = remote
        .as_ref()
        .map(|r| json!([r.kind, r.fetched_at, rows(&r.records)]));
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&json!([
        kind,
        sync,
        rows(local),
        remote
    ]))?)))
}
impl HistoryService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    pub fn snapshot_for(&self, token: &SessionToken, kind: HistoryKind) -> Result<HistorySnapshot> {
        self.session
            .with_local_owner(token, |owner| self.snapshot_for_owner(owner, kind))
    }
    /// Keep the account and history owner stable through the queue install.
    pub(crate) fn with_matching_revision<T>(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        expected: &str,
        commit: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        self.session.with_local_owner(token, |owner| {
            if self.snapshot_for_owner(owner, kind)?.revision != expected {
                return Err(Error::QueueChanged);
            }
            commit(owner)
        })
    }
    fn snapshot_for_owner(&self, owner: &str, kind: HistoryKind) -> Result<HistorySnapshot> {
        self.store.transaction(|tx| {
            let local = local(tx, owner, kind)?;
            let remote: Option<RemoteHistory> = tx
                .get(owner, NAMESPACE, &remote_key(kind))?
                .map(serde_json::from_value)
                .transpose()?;
            if let Some(remote) = &remote {
                if remote.kind != kind
                    || remote.records.len() > 50_000
                    || remote
                        .records
                        .iter()
                        .any(|r| r.origin != RecordOrigin::Remote)
                {
                    return Err(Error::State("invalid remote history snapshot"));
                }
                for row in &remote.records {
                    row.validate(kind)?;
                }
            }
            let sync_enabled = enabled(tx, owner)?;
            let entries = merge_records(
                &local,
                if sync_enabled {
                    remote
                        .as_ref()
                        .map(|r| r.records.as_slice())
                        .unwrap_or_default()
                } else {
                    &[]
                },
            );
            let revision = revision(kind, sync_enabled, &local, &remote)?;
            Ok(HistorySnapshot {
                kind,
                revision,
                sync_enabled,
                local,
                remote,
                entries,
            })
        })
    }
    pub fn set_sync_for(&self, token: &SessionToken, desired: bool) -> Result<()> {
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| {
                let current = enabled(tx, owner)?;
                write_sync_choice(tx, owner, current, desired)?;
                if current != desired {
                    advance_sync_revision(tx, owner, SYNC_VIEW_REVISION)?;
                }
                Ok(())
            })
        })
    }
    /// M/147: update only the default, after a successful authenticated signed
    /// assignment. The native caller holds the verified account/API fence;
    /// explicit user choices and legacy settings share the same key.
    pub(crate) fn apply_verified_sync_default(
        store: &StateStore,
        owner: &str,
        assigned: bool,
    ) -> Result<Option<HistorySyncState>> {
        store.transaction(|tx| {
            if stored_sync_bool(tx, owner, SYNC_CHOICE)?.is_some() {
                return Ok(None);
            }
            let previous = stored_sync_bool(tx, owner, SYNC_DEFAULT)?;
            if previous == Some(assigned) {
                return Ok(None);
            }
            tx.put(owner, NAMESPACE, SYNC_DEFAULT, &json!(assigned))?;
            if previous.unwrap_or(true) == assigned {
                return Ok(None);
            }
            advance_sync_revision(tx, owner, SYNC_PRIVACY_REVISION)?;
            Ok(Some(HistorySyncState {
                enabled: assigned,
                first_disable_confirmed: stored_sync_bool(tx, owner, SYNC_FIRST_OFF)?
                    .unwrap_or(false),
                revision: advance_sync_revision(tx, owner, SYNC_VIEW_REVISION)?,
            }))
        })
    }

    /// A dispatch authorization is valid only for this continuous enabled period.
    pub(crate) fn enabled_sync_revision_for(&self, token: &SessionToken) -> Result<Option<u64>> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                if enabled(tx, owner)? {
                    Ok(Some(sync_privacy_revision(tx, owner)?))
                } else {
                    Ok(None)
                }
            })
        })
    }
    /// Device-local display policy; this operation does not load any history rows or API.
    pub fn sync_setting_for(
        &self,
        token: &SessionToken,
        action: HistorySyncAction,
    ) -> Result<HistorySyncState> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let previous = enabled(tx, owner)?;
                let confirmed_before =
                    stored_sync_bool(tx, owner, SYNC_FIRST_OFF)?.unwrap_or(false);
                let mut current = previous;
                let mut confirmed = confirmed_before;
                if let HistorySyncAction::Set {
                    enabled: desired,
                    confirmed_first_disable,
                } = action
                {
                    if !desired && !confirmed {
                        if current && !confirmed_first_disable {
                            return Err(Error::State("confirm local-only recent play first"));
                        }
                        // The cohort may already have turned sync off while the
                        // modal was open. Only the actual confirmation consumes it.
                        if confirmed_first_disable {
                            tx.put(owner, NAMESPACE, SYNC_FIRST_OFF, &json!(true))?;
                            confirmed = true;
                        }
                    }
                    write_sync_choice(tx, owner, current, desired)?;
                    current = desired;
                }
                Ok(HistorySyncState {
                    enabled: current,
                    first_disable_confirmed: confirmed,
                    revision: if current != previous || confirmed != confirmed_before {
                        advance_sync_revision(tx, owner, SYNC_VIEW_REVISION)?
                    } else {
                        stored_sync_revision(tx, owner, SYNC_VIEW_REVISION)?
                    },
                })
            })
        })
    }
    pub async fn refresh_for(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
    ) -> Result<HistorySnapshot> {
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            NAMESPACE,
            &format!("{owner}:{}", kind.key()),
        )?;
        let sync_revision = self
            .enabled_sync_revision_for(token)?
            .ok_or(Error::State("multi-device history is disabled"))?;
        let records = self
            .session
            .run_for(token, |api, _| async move {
                if self.enabled_sync_revision_for(token)? != Some(sync_revision) {
                    return Err(Error::StaleResponse);
                }
                HistoryApi::new(api).recent(kind).await
            })
            .await?;
        let remote = RemoteHistory {
            kind,
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            records,
        };
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                if !enabled(tx, owner)? || sync_privacy_revision(tx, owner)? != sync_revision {
                    return Err(Error::StaleResponse);
                }
                tx.put(
                    owner,
                    NAMESPACE,
                    &remote_key(kind),
                    &serde_json::to_value(&remote)?,
                )
            })
        })?;
        self.snapshot_for(token, kind)
    }
    /// Atomic per-account upsert and official 300/100-item eviction. Older
    /// delayed native events cannot move an already newer record backwards.
    pub fn record_for(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        record: HistoryRecord,
    ) -> Result<()> {
        record.validate(kind)?;
        if record.origin != RecordOrigin::Local {
            return Err(Error::InvalidInput(
                "local recorder requires a local record",
            ));
        }
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| upsert(tx, owner, kind, record))
        })
    }
}
