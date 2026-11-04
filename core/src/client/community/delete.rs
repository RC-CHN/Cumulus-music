//! M/622 event menu, M/3085 remove, M/232 delete: an explicitly confirmed own note.
use super::{AccountData, CommunityHost, Locator, MusicClient, handle, project};
use crate::{
    Error, Result,
    account::SessionToken,
    error::code_matches,
    storage::{OperationLock, StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

const JOURNAL: &str = "community-note-delete-v1";
const DELETE: &str = "/api/event/delete";

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommunityDeleteAction {
    Inspect,
    Confirm { confirmation: String },
    Check,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Record {
    Pending,
    Acknowledged,
    ObservedRemoved,
    Rejected,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ready,
    NotSent,
    Uncertain,
    StillVisible,
    Unavailable,
    Acknowledged,
    ObservedRemoved,
    Rejected,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDeleteReply {
    status: Status,
    retired: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirmation: Option<String>,
}

fn key(event: &str) -> &str {
    event.trim_start_matches('0')
}

fn read(tx: &StateTransaction<'_>, owner: &str, event: &str) -> Result<Option<Record>> {
    tx.get(owner, JOURNAL, key(event))?
        .map(serde_json::from_value)
        .transpose()
        .map_err(Error::from)
}

pub(super) fn removed(tx: &StateTransaction<'_>, owner: &str, event: &str) -> Result<bool> {
    Ok(matches!(
        read(tx, owner, event)?,
        Some(Record::Acknowledged | Record::ObservedRemoved)
    ))
}

fn existing(record: Option<Record>) -> Status {
    match record {
        None => Status::Ready,
        Some(Record::Pending) => Status::Uncertain,
        Some(Record::Acknowledged) => Status::Acknowledged,
        Some(Record::ObservedRemoved) => Status::ObservedRemoved,
        Some(Record::Rejected) => Status::Rejected,
    }
}

fn observe(reply: &Value, locator: &Locator) -> Status {
    let raw = &reply["event"];
    if !code_matches(&reply["code"], 200)
        || !raw["anonymityInfo"].is_null()
        || project::id(&raw["id"]).as_ref() != Some(&locator.event)
        || project::id(&raw["user"]["userId"]).as_ref() != Some(&locator.owner)
    {
        return Status::Unavailable;
    }
    if project::tombstone(raw) {
        Status::ObservedRemoved
    } else if project::card(raw, true)
        .and_then(|(_, owner, _)| owner)
        .as_ref()
        == Some(locator)
    {
        Status::StillVisible
    } else {
        Status::Unavailable
    }
}

impl MusicClient {
    pub(super) fn note_removed(&self, owner: &str, event: &str) -> Result<bool> {
        match self.session.persistent_store() {
            Ok(store) => store.read_snapshot(|tx| removed(tx, owner, event)),
            Err(Error::State("no persistent account store")) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn note_visible_rows<'a>(
        &self,
        owner: &str,
        rows: &'a [Value],
    ) -> Result<Vec<&'a Value>> {
        let store = match self.session.persistent_store() {
            Ok(store) => store,
            Err(Error::State("no persistent account store")) => return Ok(rows.iter().collect()),
            Err(error) => return Err(error),
        };
        store.read_snapshot(|tx| {
            let mut visible = Vec::with_capacity(rows.len());
            for row in rows {
                if row["anonymityInfo"].is_null()
                    && project::id(&row["user"]["userId"]).as_deref() == Some(owner)
                    && let Some(event) = project::id(&row["id"])
                    && removed(tx, owner, &event)?
                {
                    continue;
                }
                visible.push(row);
            }
            Ok(visible)
        })
    }

    fn note_sources(&self) -> [&CommunityHost; 4] {
        [
            &self.community,
            &self.profile_notes,
            &self.published_notes,
            &self.community_topics.cards,
        ]
    }

    fn note_delete_locator(
        &self,
        token: &SessionToken,
        detail: Uuid,
        owner: &str,
    ) -> Result<Locator> {
        self.community_host_for(token, detail)?
            .state(token)?
            .cards
            .iter()
            .find(|card| card.key == detail)
            .and_then(|card| card.locator.clone())
            .filter(|locator| locator.owner == owner)
            .ok_or(Error::StaleResponse)
    }

    fn retire_deleted_note(&self, token: &SessionToken, locator: &Locator) -> Result<Vec<String>> {
        let mut retired = Vec::new();
        for host in self.note_sources() {
            host.state(token)?.cards.retain(|card| {
                if card.locator.as_ref().is_some_and(|other| {
                    other.owner == locator.owner && key(&other.event) == key(&locator.event)
                }) {
                    retired.push(card.key);
                    false
                } else {
                    true
                }
            });
        }
        self.community_topics.retire_deleted(token, &retired)?;
        Ok(retired.into_iter().map(|key| key.to_string()).collect())
    }

    fn settle_note_delete(
        &self,
        token: &SessionToken,
        store: &StateStore,
        locator: &Locator,
        status: Status,
    ) -> Result<CommunityDeleteReply> {
        self.session.with_token(token, |owner| {
            let record = match status {
                Status::Acknowledged => Some(Record::Acknowledged),
                Status::ObservedRemoved => Some(Record::ObservedRemoved),
                Status::Rejected => Some(Record::Rejected),
                _ => None,
            };
            if record.is_some() || matches!(status, Status::NotSent) {
                store.transaction(|tx| {
                    if read(tx, owner, &locator.event)? != Some(Record::Pending) {
                        return Err(Error::StaleResponse);
                    }
                    if let Some(record) = record {
                        tx.put(
                            owner,
                            JOURNAL,
                            key(&locator.event),
                            &serde_json::to_value(record)?,
                        )
                    } else {
                        tx.delete(owner, JOURNAL, key(&locator.event))?;
                        Ok(())
                    }
                })?;
            }
            let retired = if matches!(status, Status::Acknowledged | Status::ObservedRemoved) {
                self.retire_deleted_note(token, locator)?
            } else {
                Vec::new()
            };
            Ok(CommunityDeleteReply {
                status,
                retired,
                confirmation: None,
            })
        })
    }

    pub async fn community_note_delete(
        &self,
        epoch: &str,
        detail: &str,
        action: CommunityDeleteAction,
    ) -> Result<AccountData<CommunityDeleteReply>> {
        let token = self.playlist_token(epoch)?;
        let detail = handle(detail)?;
        let store = self.session.persistent_store()?;
        let locator = self.session.with_token(&token, |owner| {
            self.note_delete_locator(&token, detail, owner)
        })?;
        let wrap = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        let read_current = || {
            self.session.with_token(&token, |owner| {
                self.note_delete_locator(&token, detail, owner)?;
                store.read_snapshot(|tx| read(tx, owner, &locator.event))
            })
        };
        if matches!(action, CommunityDeleteAction::Inspect) {
            let prior = read_current()?;
            let status = existing(prior);
            let retired = if matches!(status, Status::Acknowledged | Status::ObservedRemoved) {
                self.session
                    .with_token(&token, |_| self.retire_deleted_note(&token, &locator))?
            } else {
                Vec::new()
            };
            if !retired.is_empty() {
                return Ok(wrap(CommunityDeleteReply {
                    status,
                    retired,
                    confirmation: None,
                }));
            }
            let confirmation = self.session.with_token(&token, |_| {
                let host = self.community_host_for(&token, detail)?;
                let mut state = host.state(&token)?;
                let ticket = prior.is_none().then(Uuid::new_v4);
                state.delete_confirmation = ticket.map(|ticket| (detail, ticket, Instant::now()));
                Ok(ticket.map(|ticket| ticket.to_string()))
            })?;
            return Ok(wrap(CommunityDeleteReply {
                status,
                retired,
                confirmation,
            }));
        }
        let lock_key = format!("{}:{}", locator.owner, key(&locator.event));
        let _lock = OperationLock::acquire(store.directory(), JOURNAL, &lock_key)?;
        let record = read_current()?;
        match (action, record) {
            (CommunityDeleteAction::Confirm { confirmation }, None) => {
                let confirmation = handle(&confirmation)?;
                self.session.with_token(&token, |owner| {
                    self.note_delete_locator(&token, detail, owner)?;
                    let host = self.community_host_for(&token, detail)?;
                    let mut state = host.state(&token)?;
                    if !state
                        .delete_confirmation
                        .is_some_and(|(source, ticket, created)| {
                            source == detail
                                && ticket == confirmation
                                && created.elapsed() <= Duration::from_secs(120)
                        })
                    {
                        return Err(Error::StaleResponse);
                    }
                    state.delete_confirmation = None;
                    drop(state);
                    store.transaction(|tx| {
                        if read(tx, owner, &locator.event)?.is_some() {
                            return Err(Error::MutationUncertain);
                        }
                        tx.put(
                            owner,
                            JOURNAL,
                            key(&locator.event),
                            &serde_json::to_value(Record::Pending)?,
                        )
                    })
                })?;
                let expected = &token;
                let event = &locator.event;
                let result = self
                    .session
                    .run_for(&token, |api, _| async move {
                        // Check the exact lease again after journal creation and before dispatch.
                        self.session
                            .with_token(expected, |owner| {
                                self.note_delete_locator(expected, detail, owner)
                            })
                            .map_err(|error| Error::RequestNotSent(Box::new(error)))?;
                        api.request(DELETE, json!({"id": event}), false).await
                    })
                    .await;
                let status = match result {
                    Ok(reply) if code_matches(&reply["code"], 200) => Status::Acknowledged,
                    Ok(reply) if code_matches(&reply["code"], 403) => Status::Rejected,
                    Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                        Status::NotSent
                    }
                    _ => Status::Uncertain,
                };
                Ok(wrap(
                    self.settle_note_delete(&token, &store, &locator, status)?,
                ))
            }
            (CommunityDeleteAction::Check, Some(Record::Pending)) => {
                let path = format!("/api/v1/event/{}/{}", locator.owner, locator.event);
                let result = self
                    .session
                    .run_for(&token, |api, _| async move {
                        api.request(&path, json!({}), false).await
                    })
                    .await;
                let status = match result {
                    Ok(reply) => observe(&reply, &locator),
                    Err(error)
                        if matches!(
                            error.root(),
                            Error::HttpStatus {
                                status: 403 | 404 | 410,
                                ..
                            }
                        ) =>
                    {
                        Status::Unavailable
                    }
                    Err(_) => Status::Uncertain,
                };
                Ok(wrap(
                    self.settle_note_delete(&token, &store, &locator, status)?,
                ))
            }
            (CommunityDeleteAction::Check, None) => Ok(wrap(CommunityDeleteReply {
                status: Status::NotSent,
                retired: Vec::new(),
                confirmation: None,
            })),
            (_, previous) => {
                let status = existing(previous);
                let retired = if matches!(status, Status::Acknowledged | Status::ObservedRemoved) {
                    self.session
                        .with_token(&token, |_| self.retire_deleted_note(&token, &locator))?
                } else {
                    Vec::new()
                };
                Ok(wrap(CommunityDeleteReply {
                    status,
                    retired,
                    confirmation: None,
                }))
            }
        }
    }
}
