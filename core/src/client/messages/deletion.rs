//! A bounded account-scoped observation authorizes one explicitly confirmed
//! private deletion. An ambiguous submission is permanently readback-only.
use super::{MessagePage, MessageQuery, MessageTab, MusicClient, endpoint, project, projection};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

const CONVERSATION: &str = "/api/msg/private/delete";
const SINGLE: &str = "/api/msg/private/delete/single";
const JOURNAL: &str = "private-message-deletion-v1";
// The drawer retains at most 300 rows in a list and 300 in the current thread.
const MAX_OBSERVED: usize = 1_200;
const MAX_PENDING: usize = 300;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeleteAction {
    Remove,
    Check,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeleteOutcome {
    Removed,
    Uncertain,
    StillListed,
    NotInPage,
    ReadUnavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePendingDeletion {
    receipt: String,
    conversation: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Target {
    Conversation {
        peer: String,
        wire: Value,
    },
    Single {
        peer: String,
        id: String,
        wire: Value,
    },
}

impl Target {
    fn peer(&self) -> &str {
        match self {
            Self::Conversation { peer, .. } | Self::Single { peer, .. } => peer,
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Conversation { peer, .. } => format!("peer:{peer}"),
            Self::Single { peer, id, .. } => format!("item:{peer}:{id}"),
        }
    }

    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Conversation { peer, .. }, Self::Conversation { peer: other, .. }) => {
                peer == other
            }
            (
                Self::Single { peer, id, .. },
                Self::Single {
                    peer: other,
                    id: other_id,
                    ..
                },
            ) => peer == other && id == other_id,
            _ => false,
        }
    }

    fn is_conversation(&self) -> bool {
        matches!(self, Self::Conversation { .. })
    }

    fn conflicts(&self, other: &Self) -> bool {
        self.same(other)
            || (self.peer() == other.peer() && (self.is_conversation() || other.is_conversation()))
    }

    fn write(&self) -> (&'static str, Value) {
        match self {
            Self::Conversation { wire, .. } => (CONVERSATION, json!({"userId":wire})),
            Self::Single { wire, .. } => (SINGLE, json!({"id":wire})),
        }
    }

    fn read(&self) -> MessageQuery {
        MessageQuery {
            tab: if self.is_conversation() {
                MessageTab::Private
            } else {
                MessageTab::Thread
            },
            cursor: None,
            peer_id: (!self.is_conversation()).then(|| self.peer().to_owned()),
        }
    }
}

/// The projection preserves order and length, but its fallback IDs are never
/// authorization. Capture the actual observed decimal ID and its JSON type.
pub(super) fn observed(query: &MessageQuery, response: &Value, uid: &str) -> Vec<Option<Target>> {
    if !matches!(query.tab, MessageTab::Private | MessageTab::Thread) {
        return Vec::new();
    }
    response["msgs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let user = &row["fromUser"];
            if query.tab == MessageTab::Private {
                let wire = if projection::positive_id(&user["userId"]).is_some() {
                    &user["userId"]
                } else {
                    &user["id"]
                };
                let peer = projection::positive_id(wire).filter(|peer| peer != uid)?;
                Some(Target::Conversation {
                    peer,
                    wire: wire.clone(),
                })
            } else {
                let peer = query.peer_id.as_deref().filter(|peer| *peer != uid)?;
                let from = projection::user_id(user)?;
                if from != peer && from != uid {
                    return None;
                }
                if let Some(to) = projection::user_id(&row["toUser"])
                    && (if from == uid { to != peer } else { to != uid })
                {
                    return None;
                }
                let id = projection::positive_id(&row["id"])?;
                Some(Target::Single {
                    peer: peer.to_owned(),
                    id,
                    wire: row["id"].clone(),
                })
            }
        })
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    receipt: String,
    target: Target,
}

fn valid_receipt(receipt: &str) -> bool {
    receipt.len() == 32 && receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pending(store: &StateStore, uid: &str) -> Result<Vec<Pending>> {
    decode_pending(store.values(uid, JOURNAL)?, uid)
}

fn decode_pending(records: BTreeMap<String, Value>, uid: &str) -> Result<Vec<Pending>> {
    if records.len() > MAX_PENDING {
        return Err(Error::State(
            "invalid private message deletion journal size",
        ));
    }
    records
        .into_iter()
        .map(|(key, value)| {
            let record: Pending = serde_json::from_value(value)?;
            if !valid_receipt(&record.receipt)
                || record.target.key() != key
                || projection::positive_id(&json!(record.target.peer())).is_none()
                || record.target.peer() == uid
                || match &record.target {
                    Target::Conversation { peer, wire } => {
                        projection::positive_id(wire).as_ref() != Some(peer)
                    }
                    Target::Single { id, wire, .. } => {
                        projection::positive_id(wire).as_ref() != Some(id)
                    }
                }
            {
                return Err(Error::State("invalid private message deletion journal"));
            }
            Ok(record)
        })
        .collect()
}

#[derive(Default)]
pub(in crate::client) struct DeletionHost(Mutex<Observations>);

#[derive(Default)]
struct Observations {
    owner: Option<SessionToken>,
    revision: u64,
    rows: VecDeque<Pending>,
}

impl Observations {
    fn owns_peer(&self, target: &Target) -> bool {
        self.rows
            .iter()
            .any(|row| row.target.is_conversation() && row.target.peer() == target.peer())
    }
}

impl DeletionHost {
    pub(super) fn revision(&self) -> Result<u64> {
        Ok(self.0.lock().map_err(|_| Error::LockPoisoned)?.revision)
    }

    pub(super) fn publish(
        &self,
        store: &StateStore,
        token: &SessionToken,
        uid: &str,
        revision: u64,
        page: &mut MessagePage,
        observed: Vec<Option<Target>>,
    ) -> Result<()> {
        let journal = pending(store, uid)?;
        let mut snapshot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if snapshot.revision != revision {
            // A read already in flight when deletion was acknowledged can
            // display its snapshot, but cannot mint a new destructive handle.
            return Ok(());
        }
        if snapshot.owner.as_ref() != Some(token) {
            snapshot.rows.clear();
            snapshot.owner = Some(token.clone());
        }
        for (row, target) in page.rows.iter_mut().zip(observed) {
            let Some(target) = target else { continue };
            if !target.is_conversation() && !snapshot.owns_peer(&target) {
                continue;
            }
            let receipt = journal.iter().find(|entry| entry.target.same(&target));
            let existing = snapshot
                .rows
                .iter()
                .position(|entry| entry.target.same(&target));
            let previous = existing.and_then(|index| snapshot.rows.remove(index));
            let receipt_value = receipt.or(previous.as_ref()).map_or_else(
                || uuid::Uuid::new_v4().simple().to_string(),
                |entry| entry.receipt.clone(),
            );
            row.deletion_pending = receipt.is_some();
            row.deletion_receipt = Some(receipt_value.clone());
            snapshot.rows.push_back(Pending {
                receipt: receipt_value,
                target,
            });
            if snapshot.rows.len() > MAX_OBSERVED {
                snapshot.rows.pop_front();
            }
        }
        Ok(())
    }

    fn require(&self, token: &SessionToken, receipt: &str) -> Result<Target> {
        let snapshot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if snapshot.owner.as_ref() == Some(token)
            && let Some(row) = snapshot.rows.iter().find(|row| row.receipt == receipt)
            && (row.target.is_conversation() || snapshot.owns_peer(&row.target))
        {
            return Ok(row.target.clone());
        }
        Err(Error::StaleResponse)
    }

    fn forget(&self, token: &SessionToken, target: &Target) -> Result<()> {
        let mut snapshot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if snapshot.owner.as_ref() == Some(token) {
            snapshot.revision = snapshot.revision.wrapping_add(1);
            snapshot.rows.retain(|row| {
                !(target.same(&row.target)
                    || target.is_conversation() && target.peer() == row.target.peer())
            });
        }
        Ok(())
    }
}

fn definitely_rejected(error: &Error) -> bool {
    match error.root() {
        Error::RequestNotSent(_) => true,
        Error::Service { code, .. } => code
            .as_i64()
            .or_else(|| code.as_str()?.parse().ok())
            .is_some_and(|code| (400..500).contains(&code)),
        _ => false,
    }
}

impl MusicClient {
    pub async fn pending_message_deletions(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<MessagePendingDeletion>>> {
        let token = self.player_token(epoch)?;
        let store = self.session.persistent_store()?;
        let value = self.session.with_token(&token, |uid| {
            Ok(pending(&store, uid)?
                .into_iter()
                .map(|row| MessagePendingDeletion {
                    receipt: row.receipt,
                    conversation: row.target.is_conversation(),
                })
                .collect())
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn delete_message(
        &self,
        epoch: &str,
        receipt: &str,
        action: MessageDeleteAction,
    ) -> Result<AccountData<MessageDeleteOutcome>> {
        use MessageDeleteAction::{Check, Remove};
        use MessageDeleteOutcome::{NotInPage, ReadUnavailable, Removed, StillListed, Uncertain};

        if !valid_receipt(receipt) {
            return Err(Error::InvalidInput("invalid private deletion receipt"));
        }
        let token = self.player_token(epoch)?;
        let store = self.session.persistent_store()?;
        let (uid, target) = self.session.with_token(&token, |uid| {
            let target = match action {
                Remove => self.message_deletion.require(&token, receipt)?,
                Check => {
                    pending(&store, uid)?
                        .into_iter()
                        .find(|row| row.receipt == receipt)
                        .ok_or(Error::StaleResponse)?
                        .target
                }
            };
            Ok((uid.to_owned(), target))
        })?;
        let _lock = OperationLock::acquire(
            store.directory(),
            JOURNAL,
            &format!("{uid}:{}", target.peer()),
        )?;
        let key = target.key();
        let value = match action {
            Remove => {
                let ready = self.session.with_token(&token, |uid| {
                    self.message_deletion.require(&token, receipt)?;
                    store.transaction(|tx| {
                        let journal = decode_pending(tx.values(uid, JOURNAL)?, uid)?;
                        if journal.iter().any(|row| row.target.same(&target)) {
                            return Ok(false);
                        }
                        if journal.iter().any(|row| row.target.conflicts(&target)) {
                            return Err(Error::State(
                                "an overlapping private deletion is unresolved",
                            ));
                        }
                        if journal.len() >= MAX_PENDING {
                            return Err(Error::State("private deletion journal is full"));
                        }
                        tx.put(
                            uid,
                            JOURNAL,
                            &key,
                            &serde_json::to_value(Pending {
                                receipt: receipt.to_owned(),
                                target: target.clone(),
                            })?,
                        )?;
                        Ok(true)
                    })
                })?;
                if !ready {
                    Uncertain
                } else {
                    let (path, body) = target.write();
                    let result = self
                        .session
                        .run_for(&token, |api, _| async move {
                            api.call(path, body, false, &[200]).await
                        })
                        .await;
                    self.session.with_token(&token, |uid| match result {
                        Ok(_) => {
                            self.message_deletion.forget(&token, &target)?;
                            store.delete(uid, JOURNAL, &key)?;
                            Ok(Removed)
                        }
                        Err(error) if definitely_rejected(&error) => {
                            store.delete(uid, JOURNAL, &key)?;
                            Err(error)
                        }
                        Err(_) => Ok(Uncertain),
                    })?
                }
            }
            Check => {
                self.session.with_token(&token, |uid| {
                    if pending(&store, uid)?
                        .iter()
                        .any(|row| row.receipt == receipt && row.target.same(&target))
                    {
                        Ok(())
                    } else {
                        Err(Error::StaleResponse)
                    }
                })?;
                let query = target.read();
                let result = self
                    .session
                    .run_for(&token, |api, uid| async move {
                        let (path, args, limit) = endpoint(&query, &uid)?;
                        let response = api.call(&path, args, false, &[200]).await?;
                        project(&query, &response, &uid, limit)?;
                        Ok(observed(&query, &response, &uid))
                    })
                    .await;
                self.session.check_generation(&token)?;
                match result {
                    Ok(ref rows) if rows.iter().flatten().any(|row| row.same(&target)) => {
                        StillListed
                    }
                    Ok(_) => NotInPage,
                    Err(_) => ReadUnavailable,
                }
            }
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
