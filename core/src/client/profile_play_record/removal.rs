//! Only an authenticated own-list snapshot can authorize a rank mutation.
use super::{MusicClient, ProfilePlayRecordPeriod as Period, ProfilePlayRecordView as View, read};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Mutex};

const REMOVE: &str = "/api/play/record/empty";
const JOURNAL: &str = "profile-play-record-removal-v1";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePlayRecordRemovalAction {
    Remove,
    Check,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePlayRecordRemovalOutcome {
    Removed,
    Uncertain,
    NotListed,
}

#[derive(Default)]
pub(in crate::client) struct RemovalHost(Mutex<[Option<Snapshot>; 2]>);

struct Snapshot {
    token: SessionToken,
    revision: String,
    songs: HashSet<String>,
}

fn period_key(period: Period) -> &'static str {
    match period {
        Period::Week => "1",
        Period::All => "0",
    }
}

fn slot(period: Period) -> usize {
    period.fields().0 as usize
}

pub(super) fn lock_period(store: &StateStore, uid: &str, period: Period) -> Result<OperationLock> {
    OperationLock::acquire(
        store.directory(),
        JOURNAL,
        &format!("{uid}:{}", period_key(period)),
    )
}

fn journal_key(period: Period, song: &str) -> String {
    format!("{}:{song}", period_key(period))
}

fn pending(record: Option<Value>) -> Result<bool> {
    match record {
        None => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        Some(_) => Err(Error::State("invalid rank removal journal")),
    }
}

fn definitely_rejected(error: &Error) -> bool {
    match error.root() {
        Error::RequestNotSent(_) => true,
        Error::Service { code, .. } => code
            .as_i64()
            .or_else(|| code.as_str()?.parse().ok())
            .is_some_and(|value| (400..500).contains(&value)),
        _ => false,
    }
}

impl RemovalHost {
    pub(super) fn invalidate(&self, period: Period) -> Result<()> {
        self.0.lock().map_err(|_| Error::LockPoisoned)?[slot(period)] = None;
        Ok(())
    }

    pub(super) fn publish(
        &self,
        store: &StateStore,
        token: &SessionToken,
        uid: &str,
        period: Period,
        value: &mut View,
    ) -> Result<()> {
        let mut snapshots = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let snapshot = &mut snapshots[slot(period)];
        *snapshot = None;
        if let View::Ready {
            is_self: true,
            revision,
            items,
        } = value
            && !items.is_empty()
        {
            store.transaction(|tx| {
                for item in items.iter_mut() {
                    item.removal_uncertain =
                        pending(tx.get(uid, JOURNAL, &journal_key(period, &item.track.id))?)?;
                }
                Ok(())
            })?;
            let key = uuid::Uuid::new_v4().simple().to_string();
            *snapshot = Some(Snapshot {
                token: token.clone(),
                revision: key.clone(),
                songs: items.iter().map(|item| item.track.id.clone()).collect(),
            });
            *revision = Some(key);
        }
        Ok(())
    }

    fn require(
        &self,
        token: &SessionToken,
        period: Period,
        revision: &str,
        song: &str,
    ) -> Result<()> {
        if self.0.lock().map_err(|_| Error::LockPoisoned)?[slot(period)]
            .as_ref()
            .is_some_and(|current| {
                current.token == *token
                    && current.revision == revision
                    && current.songs.contains(song)
            })
        {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        }
    }

    fn forget(&self, token: &SessionToken, period: Period, song: &str) -> Result<()> {
        if let Some(current) = &mut self.0.lock().map_err(|_| Error::LockPoisoned)?[slot(period)]
            && current.token == *token
        {
            current.songs.remove(song);
        }
        Ok(())
    }
}

impl MusicClient {
    pub async fn remove_profile_play_record(
        &self,
        epoch: &str,
        period: Period,
        song_id: &str,
        revision: &str,
        action: ProfilePlayRecordRemovalAction,
    ) -> Result<AccountData<ProfilePlayRecordRemovalOutcome>> {
        use ProfilePlayRecordRemovalAction::{Check, Remove};
        use ProfilePlayRecordRemovalOutcome::{NotListed, Removed, Uncertain};

        let token = self.playlist_token(epoch)?;
        let song = identifier(&json!(song_id))?;
        if song.len() > 64 || revision.len() != 32 {
            return Err(Error::InvalidInput("invalid rank removal identity"));
        }
        let store = self.session.persistent_store()?;
        let uid = self
            .session
            .with_token(&token, |uid| identifier(&json!(uid)))?;
        let _lock = lock_period(&store, &uid, period)?;
        let key = journal_key(period, &song);
        let was_pending = self.session.with_token(&token, |uid| {
            self.profile_rank_removal
                .require(&token, period, revision, &song)?;
            let already = pending(store.get(uid, JOURNAL, &key)?)?;
            if !already && matches!(action, Remove) {
                // Persist before submitting. A process crash or lost reply
                // must never turn the next click into an implicit re-send.
                store.put(uid, JOURNAL, &key, &json!(true))?;
            }
            Ok(already)
        })?;

        let outcome = match (action, was_pending) {
            (Check, false) => return Err(Error::StaleResponse),
            (Remove, true) => Uncertain,
            (Remove, false) => {
                let response = self
                    .session
                    .run_for(&token, |api, _| {
                        let song = &song;
                        async move {
                            api.call(
                                REMOVE,
                                json!({"type":period_key(period),"songId":song}),
                                false,
                                &[200],
                            )
                            .await
                        }
                    })
                    .await;
                self.session.with_token(&token, |uid| match response {
                    Ok(_) => {
                        self.profile_rank_removal.forget(&token, period, &song)?;
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
            (Check, true) => {
                let result = self
                    .session
                    .run_for(&token, |api, uid| async move {
                        read(api.as_ref(), &uid, period, true).await
                    })
                    .await;
                self.session.with_token(&token, |uid| {
                    if let Ok(View::Ready { items, .. }) = result
                        && !items.iter().any(|item| item.track.id == song)
                    {
                        // Absence from the bounded current ranking is not
                        // proof that the earlier request committed.
                        self.profile_rank_removal.forget(&token, period, &song)?;
                        store.delete(uid, JOURNAL, &key)?;
                        return Ok(NotListed);
                    }
                    Ok(Uncertain)
                })?
            }
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: outcome,
        })
    }
}
