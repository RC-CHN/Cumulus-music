//! Device-only membership for regular owned playlists. Remote details still
//! own catalogue membership and never authorize a local file by numeric ID.
pub(super) mod reorder;
use super::{PlaylistApi, snapshot};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    library::LocalLibrary,
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
pub use reorder::{
    PlaylistMixedOrderRow, PlaylistMixedReorderEvidence, PlaylistMixedReorderReceipt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

const RECORDS: &str = "playlist-local-membership-v1";
const MAX_REMOTE: usize = 100_000; // Complete PC v6 detail contract.
const MAX_LOCAL: usize = 50_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistLocalRef {
    pub id: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistLocalChange {
    Add { rows: Vec<PlaylistLocalRef> },
    Remove { rows: Vec<PlaylistLocalRef> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaylistLocalRow {
    /// Index into the original, complete remote trackIds response. This
    /// preserves unbound server-side v=-1 rows without trusting them as files.
    Remote {
        index: usize,
    },
    Local(PlaylistLocalRef),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaylistLocalMembership {
    pub revision: String,
    pub rows: Vec<PlaylistLocalRow>,
    #[serde(skip)]
    generation: Option<u64>,
}

impl PlaylistLocalMembership {
    /// Native-only fence for a command that already read the full playlist.
    pub fn generation(&self) -> Option<u64> {
        self.generation
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistLocalMutation {
    pub changed: bool,
    pub membership: PlaylistLocalMembership,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocatedLocal {
    position: usize,
    reference: PlaylistLocalRef,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    generation: u64,
    /// A compact complete overlay: insert these rows at their absolute
    /// positions in the authoritative ordered remote list.
    local: Vec<LocatedLocal>,
}

pub struct PlaylistLocalStore {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}

struct Remote {
    writable: bool,
    revision: String,
    len: usize,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn unique_refs<'a>(
    rows: impl IntoIterator<Item = &'a PlaylistLocalRef>,
) -> Option<HashMap<&'a str, &'a str>> {
    let mut unique = HashMap::new();
    for row in rows {
        if !digest(&row.id)
            || !digest(&row.revision)
            || unique
                .insert(row.id.as_str(), row.revision.as_str())
                .is_some()
        {
            return None;
        }
    }
    Some(unique)
}

impl Remote {
    fn read(owner: &str, id: &str, detail: &Value) -> Result<Self> {
        if identifier(&detail["id"])? != id {
            return Err(Error::Response("playlist membership identity mismatch"));
        }
        // Match the ordinary playlist ownership/special-type guard. Do not
        // accept subscriber status or a renderer-supplied owner.
        let writable = identifier(&detail["userId"])
            .or_else(|_| identifier(&detail["creator"]["userId"]))
            .is_ok_and(|uid| uid == owner)
            && detail["specialType"].as_i64() != Some(5);
        let ids = detail["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing complete playlist tracks"))?;
        if ids.len() > MAX_REMOTE {
            return Err(Error::Response(
                "playlist track list exceeds complete detail limit",
            ));
        }
        Ok(Self {
            writable,
            // Keep the existing ordinary page revision unchanged until the
            // account has actually mutated its local membership.
            revision: snapshot::revision(&(id, ids))?,
            len: ids.len(),
        })
    }
}

impl Record {
    fn parse(raw: Value) -> Result<Self> {
        let record: Self = serde_json::from_value(raw)?;
        if record.generation == 0
            || record.local.len() > MAX_LOCAL
            || record
                .local
                .iter()
                .any(|row| row.position >= MAX_REMOTE + MAX_LOCAL)
            || record
                .local
                .windows(2)
                .any(|pair| pair[0].position >= pair[1].position)
            || unique_refs(record.local.iter().map(|row| &row.reference)).is_none()
        {
            return Err(Error::State("invalid local playlist membership record"));
        }
        Ok(record)
    }
}

pub(super) fn has_entries(store: &StateStore, owner: &str, id: &str) -> Result<bool> {
    Ok(store
        .get(owner, RECORDS, id)?
        .map(Record::parse)
        .transpose()?
        .is_some_and(|record| !record.local.is_empty()))
}

impl PlaylistLocalStore {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }

    pub fn project(
        &self,
        token: &SessionToken,
        id: &str,
        detail: &Value,
    ) -> Result<PlaylistLocalMembership> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let remote = Remote::read(&owner, id, detail)?;
        self.session.with_token(token, |owner| {
            let record = if remote.writable {
                self.store
                    .get(owner, RECORDS, id)?
                    .map(Record::parse)
                    .transpose()?
            } else {
                None
            };
            project(owner, id, &remote, record.as_ref())
        })
    }

    /// Recheck the captured device state without rehashing the remote order.
    /// Callers must still verify remote ordering/ownership before preparing.
    pub fn generation(&self, token: &SessionToken, id: &str) -> Result<Option<u64>> {
        identifier(&Value::String(id.to_owned()))?;
        self.session.with_token(token, |owner| {
            Ok(self
                .store
                .get(owner, RECORDS, id)?
                .map(Record::parse)
                .transpose()?
                .map(|record| record.generation))
        })
    }

    pub(crate) async fn mutate(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        change: PlaylistLocalChange,
        library: &LocalLibrary,
    ) -> Result<PlaylistLocalMutation> {
        identifier(&json!(id))?;
        let refs = match &change {
            PlaylistLocalChange::Add { rows } | PlaylistLocalChange::Remove { rows } => rows,
        };
        if !digest(expected_revision)
            || refs.is_empty()
            || refs.len() > MAX_LOCAL
            || unique_refs(refs).is_none()
        {
            return Err(Error::InvalidInput("invalid local playlist selection"));
        }
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        // Match PlaylistService's per-account writer lock so a remote write
        // in this profile cannot race the fresh ownership/order check.
        let playlist_lock =
            OperationLock::acquire(self.store.directory(), &owner, super::service::JOURNAL)?;
        reorder::require_settled(&self.store, &owner, id)?;
        let detail = self
            .session
            .run_for(token, |api, _| async move {
                PlaylistApi::new(api).detail(&json!(id)).await
            })
            .await?;
        let remote = Remote::read(&owner, id, &detail)?;
        if !remote.writable {
            return Err(Error::InvalidInput(
                "this operation requires a regular owned playlist",
            ));
        }
        let expected_revision = expected_revision.to_owned();
        let id = id.to_owned();
        let store = self.store.clone();
        let session = self.session.clone();
        let token = token.clone();
        let library = library.clone();
        // Keep both native leases through the single durable commit. No
        // lock or filesystem work is delegated to the renderer.
        tokio::task::spawn_blocking(move || {
            let _playlist_lock = playlist_lock;
            let _library_lock = match &change {
                PlaylistLocalChange::Add { rows } => Some(library.hold_playlist_additions(rows)?),
                // A disappeared/stale file can still be removed from the
                // overlay, but only by its exact persisted reference.
                PlaylistLocalChange::Remove { .. } => None,
            };
            session.with_token(&token, |owner| {
                store.transaction(|tx| {
                    let record = tx
                        .get(owner, RECORDS, &id)?
                        .map(Record::parse)
                        .transpose()?;
                    let current = project(owner, &id, &remote, record.as_ref())?;
                    if current.revision != expected_revision {
                        return Err(Error::QueueChanged);
                    }
                    let next = apply(&current.rows, change)?;
                    if next == current.rows {
                        return Ok(PlaylistLocalMutation {
                            changed: false,
                            membership: current,
                        });
                    }
                    let generation = record
                        .map_or(0, |row| row.generation)
                        .checked_add(1)
                        .ok_or(Error::State("local playlist revision exhausted"))?;
                    let record = Record {
                        generation,
                        local: next
                            .into_iter()
                            .enumerate()
                            .filter_map(|(position, row)| match row {
                                PlaylistLocalRow::Local(reference) => Some(LocatedLocal {
                                    position,
                                    reference,
                                }),
                                PlaylistLocalRow::Remote { .. } => None,
                            })
                            .collect(),
                    };
                    if record.local.len() > MAX_LOCAL {
                        return Err(Error::InvalidInput(
                            "local playlist exceeds device membership limit",
                        ));
                    }
                    let membership = project(owner, &id, &remote, Some(&record))?;
                    tx.put(owner, RECORDS, &id, &serde_json::to_value(record)?)?;
                    Ok(PlaylistLocalMutation {
                        changed: true,
                        membership,
                    })
                })
            })
        })
        .await
        .map_err(|_| Error::State("local playlist membership worker stopped"))?
    }
}

fn project(
    owner: &str,
    id: &str,
    remote: &Remote,
    record: Option<&Record>,
) -> Result<PlaylistLocalMembership> {
    let mut rows = Vec::with_capacity(remote.len + record.map_or(0, |record| record.local.len()));
    let mut fresh = (0..remote.len).peekable();
    if let Some(record) = record {
        // Official M/2997 projects local rows at their saved absolute
        // indices, clamped against the fresh list. Merge once in linear time.
        for local in &record.local {
            while rows.len() < local.position && fresh.peek().is_some() {
                rows.push(PlaylistLocalRow::Remote {
                    index: fresh.next().expect("remote index observed"),
                });
            }
            rows.push(PlaylistLocalRow::Local(local.reference.clone()));
        }
    }
    rows.extend(fresh.map(|index| PlaylistLocalRow::Remote { index }));
    let revision = match record {
        None => remote.revision.clone(),
        Some(record) => snapshot::revision(&json!([
            RECORDS,
            owner,
            id,
            remote.revision,
            record.generation
        ]))?,
    };
    Ok(PlaylistLocalMembership {
        revision,
        rows,
        generation: record.map(|record| record.generation),
    })
}

fn apply(
    current: &[PlaylistLocalRow],
    change: PlaylistLocalChange,
) -> Result<Vec<PlaylistLocalRow>> {
    let rows = match &change {
        PlaylistLocalChange::Add { rows } | PlaylistLocalChange::Remove { rows } => rows,
    };
    let wanted =
        unique_refs(rows).ok_or(Error::InvalidInput("invalid local playlist selection"))?;
    let existing = current.iter().filter_map(|row| match row {
        PlaylistLocalRow::Local(row) => Some((row.id.as_str(), row.revision.as_str())),
        PlaylistLocalRow::Remote { .. } => None,
    });
    let mut next = Vec::with_capacity(current.len() + wanted.len());
    if matches!(change, PlaylistLocalChange::Remove { .. }) {
        let present: HashMap<_, _> = existing.collect();
        if wanted
            .iter()
            .any(|(id, revision)| present.get(id) != Some(revision))
        {
            return Err(Error::QueueChanged);
        }
    } else {
        // The official success listener prepends in the submitted order and
        // removes any previous entry with the same local ID.
        next.extend(rows.iter().cloned().map(PlaylistLocalRow::Local));
    }
    next.extend(
        current
            .iter()
            .filter(|row| match row {
                PlaylistLocalRow::Remote { .. } => true,
                PlaylistLocalRow::Local(row) => !wanted.contains_key(row.id.as_str()),
            })
            .cloned(),
    );
    Ok(next)
}
