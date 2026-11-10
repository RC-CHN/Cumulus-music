//! Frozen send-once edits to a mixed playlist. Only the applicable online
//! IDs go to the service; the receipt and overlay settle in one transaction.
mod addition;
mod removal;
pub use addition::PlaylistMixedAddTarget;

use super::*;
use crate::{
    playlist::{PlaylistPhase, PlaylistService, TrackOperation},
    protocol::api::{
        GuardedPlaylistAdd, GuardedPlaylistRemove, PlaylistAddPermission, validate_response,
    },
    storage::StateTransaction,
};
use removal::Removal;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

const INTENTS: &str = "playlist-local-reorders-v1";
const REMOVALS: &str = "playlist-local-removals-v1";
const ADDITIONS: &str = "playlist-local-additions-v1";
const USED_ADDITIONS: &str = "playlist-local-addition-operations-v1";
const PATH: &str = "/api/v1/playlist/manipulate/tracks";
const REJECTIONS: &[i64] = &[400, 403, 405, 406, 502, 505, 506, 511, 512];

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistMixedOrderRow {
    Online { id: String },
    Local(PlaylistLocalRef),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistMixedReorderEvidence {
    Acknowledged,
    ChangedReadback,
    ExplicitRejection,
    NotSent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMixedReorderReceipt {
    pub playlist_id: String,
    pub operation_id: String,
    pub phase: PlaylistPhase,
    pub evidence: Option<PlaylistMixedReorderEvidence>,
    pub service_code: Option<i64>,
}

pub type PlaylistMixedRemoveReceipt = PlaylistMixedReorderReceipt;
pub type PlaylistMixedAddReceipt = PlaylistMixedReorderReceipt;

#[derive(Clone, Copy, Eq, PartialEq)]
enum EditKind {
    Reorder,
    Remove,
    Add,
}

impl EditKind {
    fn namespace(self) -> &'static str {
        match self {
            Self::Reorder => INTENTS,
            Self::Remove => REMOVALS,
            Self::Add => ADDITIONS,
        }
    }

    fn guarded(self) -> bool {
        self != Self::Reorder
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    owner: String,
    playlist: String,
    operation: String,
    before_revision: String,
    generation: u64,
    before_online: Vec<String>,
    desired_online: Vec<String>,
    desired_local: Vec<LocatedLocal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    removal: Option<Removal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    addition: Option<Vec<String>>,
    phase: PlaylistPhase,
    evidence: Option<PlaylistMixedReorderEvidence>,
    service_code: Option<i64>,
}

impl Intent {
    fn parse(owner: &str, id: &str, kind: EditKind, value: Value) -> Result<Self> {
        let row: Self = serde_json::from_value(value)?;
        let positions = row
            .desired_online
            .len()
            .saturating_add(row.desired_local.len());
        use PlaylistMixedReorderEvidence::*;
        use PlaylistPhase::*;
        let valid_phase = match (row.phase, row.evidence, row.service_code) {
            (Sending | Uncertain, None, None) | (Confirmed, Some(Acknowledged), Some(200)) => true,
            (Confirmed, Some(ChangedReadback), None) => row.before_online != row.desired_online,
            (Rejected, Some(NotSent), None) => true,
            (Rejected, Some(ExplicitRejection), Some(code)) => REJECTIONS.contains(&code),
            _ => false,
        };
        let valid_edit = match (kind, &row.removal, &row.addition) {
            (EditKind::Reorder, None, None) => {
                !row.desired_local.is_empty()
                    && online_permutation(&row.before_online, &row.desired_online)
            }
            (EditKind::Remove, Some(plan), None) => plan.valid(&row),
            (EditKind::Add, None, Some(ids)) => addition::valid(&row, ids),
            _ => false,
        };
        if row.owner != owner
            || row.playlist != id
            || !operation_id(&row.operation)
            || !digest(&row.before_revision)
            || row.generation == 0
            || row.desired_local.len() > MAX_LOCAL
            || row
                .desired_local
                .iter()
                .any(|row| row.position >= positions)
            || row
                .desired_local
                .windows(2)
                .any(|pair| pair[0].position >= pair[1].position)
            || unique_refs(row.desired_local.iter().map(|row| &row.reference)).is_none()
            || !valid_edit
            || !valid_phase
        {
            return Err(Error::State("invalid mixed playlist reorder record"));
        }
        Ok(row)
    }

    fn receipt(&self) -> PlaylistMixedReorderReceipt {
        PlaylistMixedReorderReceipt {
            playlist_id: self.playlist.clone(),
            operation_id: self.operation.clone(),
            phase: self.phase,
            evidence: self.evidence,
            service_code: self.service_code,
        }
    }
}

enum Outcome {
    Acknowledged,
    Readback,
    Rejected(i64),
    NotSent,
    Unknown,
}

enum Edit {
    Complete(Vec<String>, Vec<LocatedLocal>),
    Move {
        selected: Vec<PlaylistMixedOrderRow>,
        gap: usize,
    },
    Remove(Vec<PlaylistMixedOrderRow>),
    Add {
        selected: Vec<String>,
        operation: String,
    },
}

impl Edit {
    fn kind(&self) -> EditKind {
        match self {
            Self::Remove(_) => EditKind::Remove,
            Self::Add { .. } => EditKind::Add,
            Self::Complete(..) | Self::Move { .. } => EditKind::Reorder,
        }
    }
}

pub(in crate::playlist) fn require_settled(
    store: &StateStore,
    owner: &str,
    id: &str,
) -> Result<()> {
    for kind in [EditKind::Reorder, EditKind::Remove, EditKind::Add] {
        if store
            .get(owner, kind.namespace(), id)?
            .map(|raw| Intent::parse(owner, id, kind, raw))
            .transpose()?
            .is_some_and(|row| row.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
    }
    Ok(())
}

impl PlaylistLocalStore {
    pub(crate) async fn reorder_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        rows: Vec<PlaylistMixedOrderRow>,
    ) -> Result<PlaylistMixedReorderReceipt> {
        let (online, local) = split(rows, true)?;
        self.submit_mixed(
            token,
            id,
            expected_revision,
            Edit::Complete(online, local),
            &|| Ok(()),
        )
        .await
    }

    pub(crate) async fn move_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        selected: Vec<PlaylistMixedOrderRow>,
        gap: usize,
    ) -> Result<PlaylistMixedReorderReceipt> {
        if selected.is_empty() || selected.len() > MAX_LOCAL + MAX_REMOTE {
            return Err(Error::InvalidInput("invalid mixed playlist selection"));
        }
        self.submit_mixed(
            token,
            id,
            expected_revision,
            Edit::Move { selected, gap },
            &|| Ok(()),
        )
        .await
    }

    pub(crate) async fn remove_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        selected: Vec<PlaylistMixedOrderRow>,
    ) -> Result<PlaylistMixedRemoveReceipt> {
        if selected.is_empty() || selected.len() > MAX_LOCAL + MAX_REMOTE {
            return Err(Error::InvalidInput(
                "invalid mixed playlist removal selection",
            ));
        }
        self.submit_mixed(
            token,
            id,
            expected_revision,
            Edit::Remove(selected),
            &|| Ok(()),
        )
        .await
    }

    pub(crate) async fn add_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        ids: Vec<String>,
        operation: &str,
        source_guard: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMixedAddReceipt> {
        addition::validate_selection(&ids)?;
        if !operation_id(operation) {
            return Err(Error::InvalidInput("invalid mixed playlist operation"));
        }
        self.submit_mixed(
            token,
            id,
            expected_revision,
            Edit::Add {
                selected: ids,
                operation: operation.to_owned(),
            },
            source_guard,
        )
        .await
    }

    async fn submit_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        edit: Edit,
        source_guard: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMixedReorderReceipt> {
        identifier(&json!(id))?;
        if !digest(expected_revision) {
            return Err(Error::InvalidInput("invalid mixed playlist revision"));
        }
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lease = OperationLock::acquire(
            self.store.directory(),
            &owner,
            crate::playlist::service::JOURNAL,
        )?;
        require_settled(&self.store, &owner, id)?;
        PlaylistService::new(self.session.clone())?.require_no_pending_playlist(&owner, id)?;
        let kind = edit.kind();
        source_guard()?;
        let (detail, credential) = self.fresh_mixed_detail(token, id, kind).await?;
        let remote = Remote::read(&owner, id, &detail)?;
        require_owner(&owner, &remote, &detail, kind)?;
        let before_online = online_ids(&detail, kind)?;
        if online_set(&before_online).is_none() {
            return Err(Error::Response("invalid complete mixed playlist IDs"));
        }
        let (operation, requested, desired) = self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let current = read_local(tx, owner, id)?;
                let projected = project(owner, id, &remote, Some(&current))?;
                if expected_revision != projected.revision {
                    return Err(Error::QueueChanged);
                }
                let (desired_online, desired_local, removal, addition, proposed_operation) =
                    match edit {
                        Edit::Complete(online, local) => (online, local, None, None, None),
                        Edit::Move { selected, gap } => {
                            let (online, local) = split(
                                materialize_move(projected.rows, &before_online, selected, gap)?,
                                true,
                            )?;
                            (online, local, None, None, None)
                        }
                        Edit::Remove(selected) => {
                            let (online, local, removal) =
                                removal::project_removal(projected.rows, &before_online, selected)?;
                            (online, local, Some(removal), None, None)
                        }
                        Edit::Add {
                            selected,
                            operation,
                        } => {
                            let (online, local) =
                                addition::prepend(projected.rows, &before_online, &selected)?;
                            (online, local, None, Some(selected), Some(operation))
                        }
                    };
                if kind == EditKind::Reorder && !online_permutation(&before_online, &desired_online)
                {
                    return Err(Error::InvalidInput(
                        "mixed reorder must retain every online row",
                    ));
                }
                if !unchanged_members(&current.local, &desired_local, removal.as_ref()) {
                    return Err(Error::InvalidInput(
                        "mixed edit must retain every unselected local revision",
                    ));
                }
                if let Some(row) = read_intent(tx, owner, id, kind)? {
                    if row.phase.pending() {
                        return Err(Error::MutationUncertain);
                    }
                    if proposed_operation.as_deref() == Some(row.operation.as_str()) {
                        return Err(Error::StaleResponse);
                    }
                }
                let operation =
                    proposed_operation.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
                if kind == EditKind::Add {
                    if tx.get(owner, USED_ADDITIONS, &operation)?.is_some() {
                        return Err(Error::StaleResponse);
                    }
                    tx.put(owner, USED_ADDITIONS, &operation, &json!(id))?;
                }
                let journal = Intent {
                    owner: owner.to_owned(),
                    playlist: id.to_owned(),
                    operation: operation.clone(),
                    before_revision: projected.revision,
                    generation: current.generation,
                    before_online,
                    desired_online,
                    desired_local,
                    removal,
                    addition,
                    phase: PlaylistPhase::Sending,
                    evidence: None,
                    service_code: None,
                };
                tx.put(
                    owner,
                    kind.namespace(),
                    id,
                    &serde_json::to_value(&journal)?,
                )?;
                let requested = journal
                    .removal
                    .map(|plan| plan.online)
                    .or(journal.addition)
                    .unwrap_or_else(|| journal.desired_online.clone());
                Ok((operation, requested, journal.desired_online))
            })
        })?;
        let dispatched = AtomicBool::new(false);
        let reply = match (kind, credential) {
            (EditKind::Reorder, None) => {
                // Reorder's official array is truthy even when empty. Never
                // elide the request merely because its online part didn't move.
                let ids = requested.into_iter().map(Value::String).collect::<Vec<_>>();
                self.session
                    .run_for(token, |api, _| async move {
                        PlaylistApi::new(api)
                            .change_tracks(&json!(id), &ids, TrackOperation::Replace)
                            .await
                    })
                    .await
            }
            (EditKind::Remove | EditKind::Add, Some(credential)) => {
                self.dispatch_guarded(
                    token,
                    id,
                    requested,
                    kind,
                    &credential,
                    source_guard,
                    &dispatched,
                )
                .await
            }
            _ => Err(Error::State("mixed playlist request authority mismatch")),
        };
        let outcome = match reply {
            Ok(_) if kind.guarded() => {
                // Do not install new local positions against an old remote
                // order: confirm the exact full online projection first.
                match self.matches_online(token, &owner, id, kind, &desired).await {
                    Ok(true) => Outcome::Acknowledged,
                    Err(error) if matches!(error.root(), Error::StaleResponse) => {
                        return Err(error);
                    }
                    Ok(false) | Err(_) => Outcome::Unknown,
                }
            }
            Ok(_) => Outcome::Acknowledged,
            Err(error) if matches!(error.root(), Error::StaleResponse) => return Err(error),
            Err(_) if kind.guarded() && !dispatched.load(Ordering::Acquire) => Outcome::NotSent,
            Err(error) => classify(&error),
        };
        self.settle_mixed(token, id, kind, &operation, outcome)
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_guarded(
        &self,
        token: &SessionToken,
        id: &str,
        selected: Vec<String>,
        kind: EditKind,
        credential: &str,
        source_guard: &PlaylistAddPermission<'_>,
        dispatched: &AtomicBool,
    ) -> Result<Value> {
        self.session
            .run_for(token, |api, _| async move {
                let current = || {
                    self.session.require_api(token, &api)?;
                    if mixed_credentials(api.as_ref(), kind)?.as_deref() != Some(credential) {
                        return Err(Error::CredentialChanged);
                    }
                    source_guard()
                };
                current()?;
                let response = match kind {
                    EditKind::Remove => {
                        api.request_guarded_playlist_remove(
                            GuardedPlaylistRemove::new(id.to_owned(), selected)?,
                            credential,
                            &current,
                            dispatched,
                        )
                        .await?
                    }
                    EditKind::Add => {
                        api.request_guarded_mixed_playlist_add(
                            GuardedPlaylistAdd::new(id.to_owned(), selected)?,
                            credential,
                            &current,
                            dispatched,
                        )
                        .await?
                    }
                    EditKind::Reorder => {
                        return Err(Error::State("mixed playlist request authority mismatch"));
                    }
                };
                validate_response(PATH, response, &[200])
            })
            .await
    }

    pub(crate) fn mixed_reorder_receipt(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistMixedReorderReceipt>> {
        self.mixed_receipt(token, id, EditKind::Reorder)
    }

    pub(crate) fn mixed_remove_receipt(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistMixedRemoveReceipt>> {
        self.mixed_receipt(token, id, EditKind::Remove)
    }

    pub(crate) fn mixed_add_receipt(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistMixedAddReceipt>> {
        self.mixed_receipt(token, id, EditKind::Add)
    }

    fn mixed_receipt(
        &self,
        token: &SessionToken,
        id: &str,
        kind: EditKind,
    ) -> Result<Option<PlaylistMixedReorderReceipt>> {
        identifier(&json!(id))?;
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, kind.namespace(), id)?
                .map(|value| Intent::parse(owner, id, kind, value).map(|row| row.receipt()))
                .transpose()
        })
    }

    pub(crate) async fn reconcile_mixed_reorder(
        &self,
        token: &SessionToken,
        id: &str,
        operation: &str,
    ) -> Result<PlaylistMixedReorderReceipt> {
        self.reconcile_mixed(token, id, EditKind::Reorder, operation)
            .await
    }

    pub(crate) async fn reconcile_mixed_remove(
        &self,
        token: &SessionToken,
        id: &str,
        operation: &str,
    ) -> Result<PlaylistMixedRemoveReceipt> {
        self.reconcile_mixed(token, id, EditKind::Remove, operation)
            .await
    }

    pub(crate) async fn reconcile_mixed_add(
        &self,
        token: &SessionToken,
        id: &str,
        operation: &str,
    ) -> Result<PlaylistMixedAddReceipt> {
        self.reconcile_mixed(token, id, EditKind::Add, operation)
            .await
    }

    async fn reconcile_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        kind: EditKind,
        operation: &str,
    ) -> Result<PlaylistMixedReorderReceipt> {
        identifier(&json!(id))?;
        if !operation_id(operation) {
            return Err(Error::InvalidInput("invalid mixed playlist operation"));
        }
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lease = OperationLock::acquire(
            self.store.directory(),
            &owner,
            crate::playlist::service::JOURNAL,
        )?;
        let journal = self.session.with_token(token, |owner| {
            let row = self
                .store
                .get(owner, kind.namespace(), id)?
                .ok_or(Error::StaleResponse)?;
            let journal = Intent::parse(owner, id, kind, row)?;
            if journal.operation != operation {
                return Err(Error::StaleResponse);
            }
            Ok(journal)
        })?;
        if !journal.phase.pending() {
            return Ok(journal.receipt());
        }
        let outcome = if journal.before_online != journal.desired_online
            && self
                .matches_online(token, &owner, id, kind, &journal.desired_online)
                .await?
        {
            Outcome::Readback
        } else {
            Outcome::Unknown
        };
        self.settle_mixed(token, id, kind, operation, outcome)
    }

    async fn matches_online(
        &self,
        token: &SessionToken,
        owner: &str,
        id: &str,
        kind: EditKind,
        desired: &[String],
    ) -> Result<bool> {
        let (detail, _) = self.fresh_mixed_detail(token, id, kind).await?;
        require_owner(owner, &Remote::read(owner, id, &detail)?, &detail, kind)?;
        Ok(online_ids(&detail, kind)?.as_slice() == desired)
    }

    async fn fresh_mixed_detail(
        &self,
        token: &SessionToken,
        id: &str,
        kind: EditKind,
    ) -> Result<(Value, Option<String>)> {
        self.session
            .run_for(token, |api, _| async move {
                let credential = mixed_credentials(api.as_ref(), kind)?;
                let detail = PlaylistApi::new(api.clone()).detail(&json!(id)).await?;
                if let Some(credential) = &credential
                    && mixed_credentials(api.as_ref(), kind)?.as_deref()
                        != Some(credential.as_str())
                {
                    return Err(Error::CredentialChanged);
                }
                Ok((detail, credential))
            })
            .await
    }

    fn settle_mixed(
        &self,
        token: &SessionToken,
        id: &str,
        kind: EditKind,
        operation: &str,
        outcome: Outcome,
    ) -> Result<PlaylistMixedReorderReceipt> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let mut row = read_intent(tx, owner, id, kind)?.ok_or(Error::StaleResponse)?;
                if row.operation != operation || !row.phase.pending() {
                    return Err(Error::StaleResponse);
                }
                use PlaylistMixedReorderEvidence::*;
                use PlaylistPhase::*;
                (row.phase, row.evidence, row.service_code) = match outcome {
                    Outcome::Acknowledged => (Confirmed, Some(Acknowledged), Some(200)),
                    Outcome::Readback if row.before_online != row.desired_online => {
                        (Confirmed, Some(ChangedReadback), None)
                    }
                    Outcome::Rejected(code) => (Rejected, Some(ExplicitRejection), Some(code)),
                    Outcome::NotSent => (Rejected, Some(NotSent), None),
                    Outcome::Readback | Outcome::Unknown => (Uncertain, None, None),
                };
                if row.phase == Confirmed {
                    let current = read_local(tx, owner, id)?;
                    if current.generation != row.generation
                        || !unchanged_members(
                            &current.local,
                            &row.desired_local,
                            row.removal.as_ref(),
                        )
                    {
                        return Err(Error::QueueChanged);
                    }
                    let generation = current
                        .generation
                        .checked_add(1)
                        .ok_or(Error::State("local playlist revision exhausted"))?;
                    let overlay = Record {
                        generation,
                        local: row.desired_local.clone(),
                    };
                    tx.put(owner, RECORDS, id, &serde_json::to_value(overlay)?)?;
                }
                let receipt = row.receipt();
                tx.put(owner, kind.namespace(), id, &serde_json::to_value(row)?)?;
                Ok(receipt)
            })
        })
    }
}

fn mixed_credentials(
    api: &dyn crate::protocol::api::Api,
    kind: EditKind,
) -> Result<Option<String>> {
    let revision = match kind {
        EditKind::Reorder => return Ok(None),
        EditKind::Remove => api.local_collection_scope_revision()?,
        EditKind::Add => api.mixed_playlist_add_scope_revision()?,
    };
    Ok(Some(revision.ok_or(Error::CredentialChanged)?))
}

fn require_owner(owner: &str, remote: &Remote, detail: &Value, kind: EditKind) -> Result<()> {
    if !remote.writable || kind.guarded() && !removal::regular_owned(owner, detail) {
        return Err(Error::InvalidInput(
            "mixed edit requires a regular owned playlist",
        ));
    }
    Ok(())
}

fn read_local(tx: &StateTransaction<'_>, owner: &str, id: &str) -> Result<Record> {
    let current = tx
        .get(owner, RECORDS, id)?
        .map(Record::parse)
        .transpose()?
        .filter(|record| !record.local.is_empty())
        .ok_or(Error::State(
            "mixed reorder requires device playlist members",
        ))?;
    Ok(current)
}

fn read_intent(
    tx: &StateTransaction<'_>,
    owner: &str,
    id: &str,
    kind: EditKind,
) -> Result<Option<Intent>> {
    tx.get(owner, kind.namespace(), id)?
        .map(|row| Intent::parse(owner, id, kind, row))
        .transpose()
}

fn online_ids(detail: &Value, kind: EditKind) -> Result<Vec<String>> {
    let rows = detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing complete playlist tracks"))?;
    if kind == EditKind::Add
        && !detail["trackCount"].is_null()
        && detail["trackCount"].as_u64() != Some(rows.len() as u64)
    {
        return Err(Error::Response("incomplete mixed playlist track list"));
    }
    rows.iter()
        .map(|row| {
            if row["v"].as_i64() == Some(-1) {
                return Err(Error::State(match kind {
                    EditKind::Reorder => "cannot reorder unbound server local playlist rows",
                    EditKind::Remove => {
                        "cannot remove from a playlist with unbound server local rows"
                    }
                    EditKind::Add => "cannot add to a playlist with unbound server local rows",
                }));
            }
            identifier(&row["id"])
        })
        .collect()
}

fn materialize_move(
    rows: Vec<PlaylistLocalRow>,
    online: &[String],
    selected: Vec<PlaylistMixedOrderRow>,
    gap: usize,
) -> Result<Vec<PlaylistMixedOrderRow>> {
    let len = rows.len();
    if gap > len || selected.len() > len {
        return Err(Error::InvalidInput("mixed playlist move is out of bounds"));
    }
    let mut picked = selected.iter().collect::<HashSet<_>>();
    if picked.len() != selected.len() {
        return Err(Error::InvalidInput("duplicate mixed playlist selection"));
    }
    let mut next = Vec::with_capacity(len);
    let mut insertion = 0;
    for (index, row) in rows.into_iter().enumerate() {
        let row = identity(row, online)?;
        if !picked.remove(&row) {
            next.push(row);
            if index < gap {
                insertion += 1;
            }
        }
    }
    if !picked.is_empty() {
        return Err(Error::InvalidInput(
            "mixed playlist selection contains an unknown row",
        ));
    }
    drop(picked);
    // M/3085:2489–2505 leaves a placeholder for each selected row, splices
    // at the raw table gap, and then filters placeholders. Count only rows
    // still present before the gap, while preserving selectedData order.
    next.splice(insertion..insertion, selected);
    Ok(next)
}

fn identity(row: PlaylistLocalRow, online: &[String]) -> Result<PlaylistMixedOrderRow> {
    Ok(match row {
        PlaylistLocalRow::Remote { index } => PlaylistMixedOrderRow::Online {
            id: online.get(index).cloned().ok_or(Error::State(
                "mixed playlist online projection is incomplete",
            ))?,
        },
        PlaylistLocalRow::Local(reference) => PlaylistMixedOrderRow::Local(reference),
    })
}

fn split(
    rows: Vec<PlaylistMixedOrderRow>,
    require_local: bool,
) -> Result<(Vec<String>, Vec<LocatedLocal>)> {
    if rows.len() > MAX_LOCAL + MAX_REMOTE {
        return Err(Error::InvalidInput("mixed playlist exceeds reorder limit"));
    }
    let mut online = Vec::new();
    let mut local = Vec::new();
    for (position, row) in rows.into_iter().enumerate() {
        match row {
            PlaylistMixedOrderRow::Online { id } => online.push(id),
            PlaylistMixedOrderRow::Local(reference) => local.push(LocatedLocal {
                position,
                reference,
            }),
        }
    }
    if (require_local && local.is_empty())
        || local.len() > MAX_LOCAL
        || unique_refs(local.iter().map(|row| &row.reference)).is_none()
        || online_set(&online).is_none()
    {
        return Err(Error::InvalidInput("invalid mixed playlist rows"));
    }
    Ok((online, local))
}

fn online_set(rows: &[String]) -> Option<HashSet<&str>> {
    if rows.len() > MAX_REMOTE {
        return None;
    }
    let mut ids = HashSet::with_capacity(rows.len());
    for id in rows {
        if identifier(&Value::String(id.clone())).is_err() || !ids.insert(id.as_str()) {
            return None;
        }
    }
    Some(ids)
}

fn online_permutation(before: &[String], desired: &[String]) -> bool {
    match (online_set(before), online_set(desired)) {
        (Some(before), Some(desired)) => before == desired,
        _ => false,
    }
}

fn unchanged_members(
    before: &[LocatedLocal],
    desired: &[LocatedLocal],
    removal: Option<&Removal>,
) -> bool {
    match (
        unique_refs(before.iter().map(|row| &row.reference)),
        unique_refs(
            desired
                .iter()
                .map(|row| &row.reference)
                .chain(removal.into_iter().flat_map(|plan| &plan.local)),
        ),
    ) {
        (Some(before), Some(desired)) => before == desired,
        _ => false,
    }
}

fn operation_id(operation: &str) -> bool {
    operation.len() == 32 && operation.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn classify(error: &Error) -> Outcome {
    match error.root() {
        Error::RequestNotSent(_) => Outcome::NotSent,
        Error::Service { endpoint, .. }
        | Error::HttpStatus {
            endpoint: Some(endpoint),
            ..
        } if endpoint == PATH => REJECTIONS
            .iter()
            .copied()
            .find(|code| error.is_service_code(*code))
            .map_or(Outcome::Unknown, Outcome::Rejected),
        _ => Outcome::Unknown,
    }
}
