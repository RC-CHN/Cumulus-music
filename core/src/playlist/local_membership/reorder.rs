//! A frozen, send-once mixed reorder. The service sees only the full online
//! order; only a positive receipt or a distinguishable readback unlocks the
//! transaction that saves the desired device row positions.
use super::*;
use crate::{
    playlist::{PlaylistPhase, PlaylistService, TrackOperation},
    storage::StateTransaction,
};
use std::collections::HashSet;

const INTENTS: &str = "playlist-local-reorders-v1";
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
    phase: PlaylistPhase,
    evidence: Option<PlaylistMixedReorderEvidence>,
    service_code: Option<i64>,
}

impl Intent {
    fn parse(owner: &str, id: &str, value: Value) -> Result<Self> {
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
        if row.owner != owner
            || row.playlist != id
            || !operation_id(&row.operation)
            || !digest(&row.before_revision)
            || row.generation == 0
            || row.desired_local.is_empty()
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
            || !online_permutation(&row.before_online, &row.desired_online)
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

enum Reorder {
    Complete(Vec<String>, Vec<LocatedLocal>),
    Move {
        selected: Vec<PlaylistMixedOrderRow>,
        gap: usize,
    },
}

pub(in crate::playlist) fn require_settled(
    store: &StateStore,
    owner: &str,
    id: &str,
) -> Result<()> {
    if store
        .get(owner, INTENTS, id)?
        .map(|raw| Intent::parse(owner, id, raw))
        .transpose()?
        .is_some_and(|row| row.phase.pending())
    {
        return Err(Error::MutationUncertain);
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
        let (online, local) = split(rows)?;
        self.submit_reorder(
            token,
            id,
            expected_revision,
            Reorder::Complete(online, local),
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
        self.submit_reorder(
            token,
            id,
            expected_revision,
            Reorder::Move { selected, gap },
        )
        .await
    }

    async fn submit_reorder(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        reorder: Reorder,
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
        let detail = self.fresh_reorder_detail(token, id).await?;
        let remote = Remote::read(&owner, id, &detail)?;
        require_owner(&remote)?;
        let before_online = online_ids(&detail)?;
        let (operation, desired_online) = self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let current = read_local(tx, owner, id)?;
                let projected = project(owner, id, &remote, Some(&current))?;
                if projected.revision != expected_revision {
                    return Err(Error::QueueChanged);
                }
                let (desired_online, desired_local) = match reorder {
                    Reorder::Complete(online, local) => (online, local),
                    Reorder::Move { selected, gap } => split(materialize_move(
                        projected.rows,
                        &before_online,
                        selected,
                        gap,
                    )?)?,
                };
                if !online_permutation(&before_online, &desired_online) {
                    return Err(Error::InvalidInput(
                        "mixed reorder must retain every online row",
                    ));
                }
                if !same_members(&current.local, &desired_local) {
                    return Err(Error::InvalidInput(
                        "mixed reorder must retain every local revision",
                    ));
                }
                if read_intent(tx, owner, id)?.is_some_and(|row| row.phase.pending()) {
                    return Err(Error::MutationUncertain);
                }
                let operation = uuid::Uuid::new_v4().simple().to_string();
                let journal = Intent {
                    owner: owner.to_owned(),
                    playlist: id.to_owned(),
                    operation: operation.clone(),
                    before_revision: expected_revision.to_owned(),
                    generation: current.generation,
                    before_online,
                    desired_online,
                    desired_local,
                    phase: PlaylistPhase::Sending,
                    evidence: None,
                    service_code: None,
                };
                tx.put(owner, INTENTS, id, &serde_json::to_value(&journal)?)?;
                Ok((operation, journal.desired_online))
            })
        })?;
        // The official array is truthy even when empty. Never elide the
        // request just because no online item moved or because it is [].
        let ids = desired_online
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>();
        let reply = self
            .session
            .run_for(token, |api, _| async move {
                PlaylistApi::new(api)
                    .change_tracks(&json!(id), &ids, TrackOperation::Replace)
                    .await
            })
            .await;
        let outcome = match reply {
            Ok(_) => Outcome::Acknowledged,
            Err(error) if matches!(error.root(), Error::StaleResponse) => return Err(error),
            Err(error) => classify(&error),
        };
        self.settle_reorder(token, id, &operation, outcome)
    }

    pub(crate) fn mixed_reorder_receipt(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistMixedReorderReceipt>> {
        identifier(&json!(id))?;
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, INTENTS, id)?
                .map(|value| Intent::parse(owner, id, value).map(|row| row.receipt()))
                .transpose()
        })
    }

    pub(crate) async fn reconcile_mixed_reorder(
        &self,
        token: &SessionToken,
        id: &str,
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
                .get(owner, INTENTS, id)?
                .ok_or(Error::StaleResponse)?;
            let journal = Intent::parse(owner, id, row)?;
            if journal.operation != operation {
                return Err(Error::StaleResponse);
            }
            Ok(journal)
        })?;
        if !journal.phase.pending() {
            return Ok(journal.receipt());
        }
        let outcome = if journal.before_online == journal.desired_online {
            Outcome::Unknown
        } else {
            let detail = self.fresh_reorder_detail(token, id).await?;
            require_owner(&Remote::read(&owner, id, &detail)?)?;
            if online_ids(&detail)? == journal.desired_online {
                Outcome::Readback
            } else {
                Outcome::Unknown
            }
        };
        self.settle_reorder(token, id, operation, outcome)
    }

    async fn fresh_reorder_detail(&self, token: &SessionToken, id: &str) -> Result<Value> {
        self.session
            .run_for(token, |api, _| async move {
                PlaylistApi::new(api).detail(&json!(id)).await
            })
            .await
    }

    fn settle_reorder(
        &self,
        token: &SessionToken,
        id: &str,
        operation: &str,
        outcome: Outcome,
    ) -> Result<PlaylistMixedReorderReceipt> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let mut row = read_intent(tx, owner, id)?.ok_or(Error::StaleResponse)?;
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
                        || !same_members(&current.local, &row.desired_local)
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
                tx.put(owner, INTENTS, id, &serde_json::to_value(row)?)?;
                Ok(receipt)
            })
        })
    }
}

fn require_owner(remote: &Remote) -> Result<()> {
    if !remote.writable {
        return Err(Error::InvalidInput(
            "mixed reorder requires a regular owned playlist",
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

fn read_intent(tx: &StateTransaction<'_>, owner: &str, id: &str) -> Result<Option<Intent>> {
    tx.get(owner, INTENTS, id)?
        .map(|row| Intent::parse(owner, id, row))
        .transpose()
}

fn online_ids(detail: &Value) -> Result<Vec<String>> {
    detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing complete playlist tracks"))?
        .iter()
        .map(|row| {
            if row["v"].as_i64() == Some(-1) {
                return Err(Error::State(
                    "cannot reorder unbound server local playlist rows",
                ));
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
        let row = match row {
            PlaylistLocalRow::Remote { index } => PlaylistMixedOrderRow::Online {
                id: online.get(index).cloned().ok_or(Error::State(
                    "mixed playlist online projection is incomplete",
                ))?,
            },
            PlaylistLocalRow::Local(reference) => PlaylistMixedOrderRow::Local(reference),
        };
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

fn split(rows: Vec<PlaylistMixedOrderRow>) -> Result<(Vec<String>, Vec<LocatedLocal>)> {
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
    if local.is_empty()
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

fn same_members(before: &[LocatedLocal], desired: &[LocatedLocal]) -> bool {
    match (
        unique_refs(before.iter().map(|row| &row.reference)),
        unique_refs(desired.iter().map(|row| &row.reference)),
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
