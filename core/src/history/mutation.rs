use super::{
    HistoryApi, HistoryBoundary, HistoryKind, HistoryService, RemoteHistory,
    service::{NAMESPACE, enabled, local, local_key, remote_key, revision},
};
use crate::{Error, Result, account::SessionToken, storage::OperationLock};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const JOURNAL: &str = "play-history-mutations-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryAction {
    Delete { ids: Vec<String> },
    Clear,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMutationPhase {
    Pending,
    Confirmed,
    Rejected,
    Uncertain,
    AcceptedCurrentState,
}
impl HistoryMutationPhase {
    pub fn unresolved(self) -> bool {
        matches!(self, Self::Pending | Self::Uncertain)
    }
}
/// Persistent native intent. A UI projects this into a safe, smaller DTO.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryMutation {
    pub id: String,
    pub kind: HistoryKind,
    pub action: HistoryAction,
    pub expected_revision: String,
    pub phase: HistoryMutationPhase,
    pub created_at: u64,
    pub local_removed: usize,
    pub remote_planned: bool,
    pub remote_ids: Vec<String>,
    pub boundary: Option<HistoryBoundary>,
    pub last_read_at: Option<u64>,
}
fn id_valid(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(Error::InvalidInput("history mutation requires a UUID"));
    }
    Ok(())
}
fn now() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64
}
fn selected(action: &HistoryAction, id: &str) -> bool {
    match action {
        HistoryAction::Clear => true,
        HistoryAction::Delete { ids } => ids.iter().any(|v| v == id),
    }
}
pub(super) fn removed_since(
    tx: &crate::storage::StateTransaction<'_>,
    owner: &str,
    kind: HistoryKind,
    id: &str,
    played_at: u64,
) -> Result<bool> {
    for key in [
        format!("cleared:{}", kind.key()),
        format!("deleted:{}:{id}", kind.key()),
    ] {
        if let Some(value) = tx.get(owner, NAMESPACE, &key)? {
            let at = value
                .as_u64()
                .ok_or(Error::State("invalid history deletion boundary"))?;
            if at >= played_at {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
impl HistoryService {
    pub fn mutations_for(&self, token: &SessionToken) -> Result<Vec<HistoryMutation>> {
        self.session.with_local_owner(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_iter()
                .map(|(id, value)| {
                    let op: HistoryMutation = serde_json::from_value(value)?;
                    if op.id != id {
                        return Err(Error::State("history journal identity mismatch"));
                    }
                    id_valid(&id)?;
                    Ok(op)
                })
                .collect()
        })
    }
    pub async fn mutate_for(
        &self,
        token: &SessionToken,
        id: &str,
        kind: HistoryKind,
        expected_revision: &str,
        action: HistoryAction,
    ) -> Result<HistoryMutation> {
        id_valid(id)?;
        let owner = self
            .session
            .with_local_owner(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            NAMESPACE,
            &format!("{owner}:{}", kind.key()),
        )?;
        let prior = self.mutations_for(token)?;
        if let Some(op) = prior.iter().find(|op| op.id == id) {
            if op.kind != kind || op.action != action || op.expected_revision != expected_revision {
                return Err(Error::InvalidInput("history intent ID reused"));
            }
            // Includes Pending from a crash: inspect, never execute again.
            return Ok(op.clone());
        }
        if prior
            .iter()
            .any(|op| op.kind == kind && op.phase.unresolved())
        {
            return Err(Error::MutationUncertain);
        }
        let page = self.snapshot_for(token, kind)?;
        if page.revision != expected_revision {
            return Err(Error::State("history changed before operation"));
        }
        if let HistoryAction::Delete { ids } = &action {
            let unique = ids.iter().collect::<HashSet<_>>();
            if ids.is_empty()
                || ids.len() > 50_000
                || unique.len() != ids.len()
                || ids
                    .iter()
                    .any(|id| !page.entries.iter().any(|r| &r.id == id))
            {
                return Err(Error::InvalidInput(
                    "history selection is not in the displayed snapshot",
                ));
            }
        }
        let remote_enabled = page.sync_enabled && token.uid.is_some();
        if remote_enabled && page.remote.is_none() {
            return Err(Error::State(
                "read remote history before deleting synchronized records",
            ));
        }
        let remote_ids = match &action {
            HistoryAction::Clear => Vec::new(),
            HistoryAction::Delete { ids } => ids
                .iter()
                .filter(|id| {
                    page.remote
                        .as_ref()
                        .is_some_and(|remote| remote.records.iter().any(|r| &r.id == *id))
                })
                .cloned()
                .collect::<Vec<_>>(),
        };
        let remote_planned = remote_enabled
            && match action {
                HistoryAction::Clear => true,
                HistoryAction::Delete { .. } => !remote_ids.is_empty(),
            };
        let boundary = page
            .remote
            .as_ref()
            .and_then(|r| r.records.last())
            .map(|r| HistoryBoundary {
                id: r.id.clone(),
                played_at: r.played_at,
            });
        let mut op = HistoryMutation {
            id: id.into(),
            kind,
            action,
            expected_revision: expected_revision.into(),
            phase: if remote_planned {
                HistoryMutationPhase::Pending
            } else {
                HistoryMutationPhase::Confirmed
            },
            created_at: now(),
            local_removed: 0,
            remote_planned,
            remote_ids,
            boundary,
            last_read_at: None,
        };
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| {
                // UUID identity is account-wide, including simultaneous
                // operations on two different history kinds.
                if tx.get(owner, JOURNAL, id)?.is_some() {
                    return Err(Error::InvalidInput("history intent ID reused"));
                }
                let mut rows = local(tx, owner, kind)?;
                let remote: Option<RemoteHistory> = tx
                    .get(owner, NAMESPACE, &remote_key(kind))?
                    .map(serde_json::from_value)
                    .transpose()?;
                if revision(kind, enabled(tx, owner)?, &rows, &remote)? != expected_revision {
                    return Err(Error::State("history changed before operation"));
                }
                let before = rows.len();
                rows.retain(|r| !selected(&op.action, &r.id));
                op.local_removed = before - rows.len();
                match &op.action {
                    HistoryAction::Clear => tx.put(
                        owner,
                        NAMESPACE,
                        &format!("cleared:{}", kind.key()),
                        &serde_json::json!(op.created_at),
                    )?,
                    HistoryAction::Delete { ids } => {
                        for id in ids {
                            tx.put(
                                owner,
                                NAMESPACE,
                                &format!("deleted:{}:{id}", kind.key()),
                                &serde_json::json!(op.created_at),
                            )?;
                        }
                    }
                }
                tx.put(
                    owner,
                    NAMESPACE,
                    &local_key(kind),
                    &serde_json::to_value(&rows)?,
                )?;
                tx.put(owner, JOURNAL, id, &serde_json::to_value(&op)?)
            })
        })?;
        if !remote_planned {
            return Ok(op);
        }
        let result = self
            .session
            .run_for(token, |api, _| {
                let action = op.action.clone();
                let ids = op.remote_ids.clone();
                let boundary = op.boundary.clone();
                async move {
                    let api = HistoryApi::new(api);
                    match action {
                        HistoryAction::Clear => api.clear(kind).await,
                        HistoryAction::Delete { .. } => {
                            api.delete(
                                kind,
                                &ids,
                                &boundary
                                    .ok_or(Error::State("history deletion boundary missing"))?,
                            )
                            .await
                        }
                    }
                }
            })
            .await;
        op.phase = match &result {
            Ok(()) => HistoryMutationPhase::Confirmed,
            Err(error) => match error.root() {
                Error::Service { .. } | Error::RequestNotSent(_) => HistoryMutationPhase::Rejected,
                _ => HistoryMutationPhase::Uncertain,
            },
        };
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| {
                // The remote page is locked, but native playback may have created
                // fresh local rows meanwhile. Never delete those rows a second time.
                if result.is_ok()
                    && let Some(mut remote) = page.remote.clone()
                {
                    remote.records.retain(|r| !selected(&op.action, &r.id));
                    tx.put(
                        owner,
                        NAMESPACE,
                        &remote_key(kind),
                        &serde_json::to_value(remote)?,
                    )?;
                }
                tx.put(owner, JOURNAL, id, &serde_json::to_value(&op)?)
            })
        })?;
        Ok(op)
    }
    /// A read can update what the user sees but a bounded recent-list absence
    /// cannot prove that a lost write will never arrive. Keep the intent pending.
    pub async fn reconcile_for(&self, token: &SessionToken, id: &str) -> Result<HistoryMutation> {
        id_valid(id)?;
        let op = self
            .mutations_for(token)?
            .into_iter()
            .find(|op| op.id == id)
            .ok_or(Error::InvalidInput("unknown history operation"))?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        let page = self.refresh_for(token, op.kind).await?;
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| {
                let mut current: HistoryMutation = serde_json::from_value(
                    tx.get(owner, JOURNAL, id)?
                        .ok_or(Error::State("history journal disappeared"))?,
                )?;
                if current.phase.unresolved() {
                    current.last_read_at = page.remote.map(|r| r.fetched_at);
                    tx.put(owner, JOURNAL, id, &serde_json::to_value(&current)?)?;
                }
                // An explicit acceptance racing this read stays terminal.
                Ok(current)
            })
        })
    }
    /// Explicit user acceptance after a fresh read; it does not claim the lost
    /// remote write succeeded and does not send, undo or retry any deletion.
    pub fn accept_current_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
    ) -> Result<HistoryMutation> {
        id_valid(id)?;
        let mut op = self
            .mutations_for(token)?
            .into_iter()
            .find(|op| op.id == id)
            .ok_or(Error::InvalidInput("unknown history operation"))?;
        let owner = self
            .session
            .with_local_owner(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            NAMESPACE,
            &format!("{owner}:{}", op.kind.key()),
        )?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        if op.last_read_at.is_none() {
            return Err(Error::State(
                "read history before accepting its current state",
            ));
        }
        self.session.with_local_owner(token, |owner| {
            self.store.transaction(|tx| {
                let rows = local(tx, owner, op.kind)?;
                let remote: Option<RemoteHistory> = tx
                    .get(owner, NAMESPACE, &remote_key(op.kind))?
                    .map(serde_json::from_value)
                    .transpose()?;
                if revision(op.kind, enabled(tx, owner)?, &rows, &remote)? != expected_revision {
                    return Err(Error::State("history changed before acceptance"));
                }
                op.phase = HistoryMutationPhase::AcceptedCurrentState;
                tx.put(owner, JOURNAL, id, &serde_json::to_value(&op)?)
            })
        })?;
        Ok(op)
    }
}
