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
pub(super) fn enabled(tx: &StateTransaction<'_>, owner: &str) -> Result<bool> {
    match tx.get(owner, NAMESPACE, "sync-enabled")? {
        None => Ok(true),
        Some(Value::Bool(b)) => Ok(b),
        _ => Err(Error::State("invalid recent play preference")),
    }
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
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&json!([
        kind, sync, local, remote
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
        self.session.with_local_owner(token, |owner| {
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
        })
    }
    pub fn set_sync_for(&self, token: &SessionToken, enabled: bool) -> Result<()> {
        self.session.with_local_owner(token, |owner| {
            self.store
                .put(owner, NAMESPACE, "sync-enabled", &json!(enabled))
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
        if !self.snapshot_for(token, kind)?.sync_enabled {
            return Err(Error::State("multi-device history is disabled"));
        }
        let records = self
            .session
            .run_for(token, |api, _| async move {
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
                if !enabled(tx, owner)? {
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
