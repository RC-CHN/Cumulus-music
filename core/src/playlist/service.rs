mod operation;
mod reconcile;
use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const JOURNAL: &str = "playlist-mutations";
pub struct PlaylistService {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
fn now() -> Result<f64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .map_err(|_| Error::State("system clock precedes UNIX epoch"))
}
impl PlaylistService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |uid| Ok(uid.into()))
    }
    fn records(&self, uid: &str) -> Result<Vec<PlaylistMutation>> {
        self.store
            .values(uid, JOURNAL)?
            .into_iter()
            .map(|(key, v)| {
                let mut r: PlaylistMutation = serde_json::from_value(v)?;
                if r.operation_id != key
                    || key.len() != 32
                    || !key.bytes().all(|b| b.is_ascii_hexdigit())
                    || !r.created_at.is_finite()
                {
                    return Err(Error::Response("invalid playlist operation record"));
                }
                r.command.validate()?;
                Ok(r)
            })
            .collect()
    }
    fn persist(&self, uid: &str, record: &PlaylistMutation) -> Result<()> {
        self.store.put(
            uid,
            JOURNAL,
            &record.operation_id,
            &serde_json::to_value(record)?,
        )
    }
    pub fn operations(&self, token: &SessionToken) -> Result<Vec<PlaylistMutation>> {
        self.session.with_token(token, |uid| self.records(uid))
    }
    pub async fn list(&self, token: &SessionToken) -> Result<PlaylistList> {
        let value = self
            .session
            .run_for(token, |api, uid| async move {
                PlaylistList::read(&PlaylistApi::new(api), &uid).await
            })
            .await?;
        self.session.save_result(
            token,
            "playlist-cache",
            "list",
            &serde_json::to_value(&value)?,
        )?;
        Ok(value)
    }
    pub async fn detail(&self, token: &SessionToken, id: &str) -> Result<PlaylistSnapshot> {
        crate::protocol::api::identifier(&serde_json::json!(id))?;
        let value = self
            .session
            .run_for(token, |api, _| async move {
                PlaylistSnapshot::read(&PlaylistApi::new(api), id).await
            })
            .await?;
        self.session
            .save_result(token, "playlist-cache", id, &serde_json::to_value(&value)?)?;
        Ok(value)
    }
    pub fn cached_detail(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistSnapshot>> {
        crate::protocol::api::identifier(&serde_json::json!(id))?;
        self.session.with_token(token, |uid| {
            let Some(value) = self.store.get(uid, "playlist-cache", id)? else {
                return Ok(None);
            };
            let snapshot: PlaylistSnapshot = serde_json::from_value(value)?;
            if snapshot.summary.id != id {
                return Err(Error::State("cached playlist identity differs"));
            }
            Ok(Some(snapshot))
        })
    }
    /// At most one local writer per account. Only unresolved overlapping
    /// resources block subsequent commands; unrelated metadata stays editable.
    pub async fn execute(
        &self,
        token: &SessionToken,
        command: PlaylistCommand,
    ) -> Result<PlaylistMutation> {
        self.execute_identified(token, command, &uuid::Uuid::new_v4().simple().to_string())
            .await
    }
    /// A durable parent records this ID before invoking its child. An existing
    /// child is only read back, even after a crash between sending and reply.
    pub(super) async fn execute_identified(
        &self,
        token: &SessionToken,
        mut command: PlaylistCommand,
        operation_id: &str,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        command.validate()?;
        if operation_id.len() != 32 || !operation_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid playlist operation identifier"));
        }
        if matches!(
            &command,
            PlaylistCommand::Tracks {
                change: TrackChange::LegacyReplace,
                ..
            }
        ) {
            return Err(Error::InvalidInput(
                "update is reorder, not replacement; use clear to remove all tracks",
            ));
        }
        if matches!(&command,PlaylistCommand::Tracks{change:TrackChange::Clear,ids,..} if !ids.is_empty())
        {
            return Err(Error::InvalidInput("clear takes no caller-supplied ids"));
        }
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let records = self.records(&uid)?;
        if let Some(record) = records.iter().find(|r| r.operation_id == operation_id) {
            if serde_json::to_value(&record.command)? != serde_json::to_value(&command)? {
                return Err(Error::State(
                    "playlist operation ID belongs to another command",
                ));
            }
            return if record.phase.pending() {
                self.check(token, record.clone()).await
            } else {
                Ok(record.clone())
            };
        }
        if records.iter().any(|r| {
            r.phase.pending()
                && (r.command.conflicts(&command)
                    || r.playlist_id
                        .as_deref()
                        .zip(command.playlist_id())
                        .is_some_and(|(a, b)| a == b))
        }) {
            return Err(Error::MutationUncertain);
        }
        let before = self.prepare(token, &command).await?;
        let mut record = PlaylistMutation {
            operation_id: operation_id.into(),
            playlist_id: command.playlist_id().map(str::to_owned),
            command,
            phase: PlaylistPhase::Sending,
            before: before.clone(),
            observed: None,
            created_at: now()?,
            checked_at: None,
            codes: Default::default(),
            error_class: None,
        };
        if reconcile::matches(&mut record, &before, &uid)? {
            record.phase = PlaylistPhase::Unchanged;
            record.observed = Some(before);
        }
        self.session.save_result(
            token,
            JOURNAL,
            &record.operation_id,
            &serde_json::to_value(&record)?,
        )?;
        if record.phase == PlaylistPhase::Unchanged {
            return Ok(record);
        }
        let command = record.command.clone();
        let before = record.before.clone();
        let reply = self
            .session
            .run_for(token, |api, uid| async move {
                operation::send(PlaylistApi::new(api), &uid, command, before).await
            })
            .await;
        match reply {
            Ok(ack) => {
                record.phase = PlaylistPhase::Verifying;
                record.codes = ack.codes;
                if ack.created.is_some() {
                    record.playlist_id = ack.created;
                }
            }
            Err(error) => {
                if matches!(error.root(), Error::RequestNotSent(_)) {
                    record.phase = PlaylistPhase::Rejected;
                    record.error_class = Some("not_sent".into());
                    self.persist(&uid, &record)?;
                    return Err(error);
                }
                record.phase = PlaylistPhase::Uncertain;
                record.codes = operation::error_codes(&error);
                record.error_class = Some(
                    match error.root() {
                        Error::Transport(_) => "transport",
                        Error::Batch { .. } => "batch",
                        Error::Service { .. } | Error::HttpStatus { .. } => "service",
                        Error::StaleResponse => "stale",
                        _ => "state_or_response",
                    }
                    .into(),
                );
                // Old-account responsibility survives a late reply, but never
                // gets published into the newly selected account's cache.
                self.persist(&uid, &record)?;
                if error.is_service_code(301) || self.owner(token).is_err() {
                    return Err(error);
                }
            }
        }
        self.persist(&uid, &record)?;
        self.check(token, record).await
    }
    pub async fn reconcile(
        &self,
        token: &SessionToken,
        operation_id: &str,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let record = self
            .records(&uid)?
            .into_iter()
            .find(|r| r.operation_id == operation_id)
            .ok_or(Error::InvalidInput("unknown playlist operation"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.check(token, record).await
    }
    async fn check(
        &self,
        token: &SessionToken,
        mut record: PlaylistMutation,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let observed = match record.command {
            PlaylistCommand::Create { .. }
            | PlaylistCommand::Delete { .. }
            | PlaylistCommand::Subscribe { .. }
            | PlaylistCommand::Order { .. } => serde_json::to_value(self.list(token).await?),
            _ => serde_json::to_value(
                self.detail(token, record.command.playlist_id().expect("detail command"))
                    .await?,
            ),
        }?;
        record.phase = if reconcile::matches(&mut record, &observed, &uid)? {
            PlaylistPhase::Confirmed
        } else {
            // The first live audit proved acknowledged update([]) is a sort
            // no-op. Preserve its failure instead of indefinitely blocking
            // cleanup, or retroactively claiming the requested clear worked.
            if matches!(&record.command,PlaylistCommand::Tracks{change:TrackChange::LegacyReplace,ids,..} if ids.is_empty())
                && record.codes.get("main") == Some(&200)
                && observed["tracks"] == record.before["tracks"]
            {
                record.error_class = Some("unsupported_replace_contract".into());
                PlaylistPhase::Rejected
            } else {
                operation::unmatched_phase(&record)
            }
        };
        record.observed = Some(observed);
        record.checked_at = Some(now()?);
        self.session.save_result(
            token,
            JOURNAL,
            &record.operation_id,
            &serde_json::to_value(&record)?,
        )?;
        // Return the actual readback even for a partial/rejected/uncertain
        // result, so hosts cannot accidentally paint an optimistic success.
        Ok(record)
    }
    async fn prepare(&self, token: &SessionToken, command: &PlaylistCommand) -> Result<Value> {
        let uid = self.owner(token)?;
        match command {
            PlaylistCommand::Create { .. }
            | PlaylistCommand::Order { .. }
            | PlaylistCommand::Subscribe { .. } => {
                let list = self.list(token).await?;
                if let PlaylistCommand::Order {
                    group,
                    revision,
                    ids,
                } = command
                {
                    if revision != &list.revision {
                        return Err(Error::QueueChanged);
                    }
                    operation::permutation(&list.group(&uid, *group), ids)?;
                    for p in list
                        .playlists
                        .iter()
                        .filter(|p| p.favorite && p.owner == uid)
                    {
                        let original = list.group(&uid, *group);
                        if original.iter().position(|id| id == &p.id)
                            != ids.iter().position(|id| id == &p.id)
                        {
                            return Err(Error::InvalidInput("favorite playlist position is fixed"));
                        }
                    }
                }
                if let PlaylistCommand::Subscribe { id, subscribed } = command {
                    let owner = if let Some(p) = list.playlists.iter().find(|p| p.id == *id) {
                        Some(p.owner.clone())
                    } else if *subscribed {
                        Some(self.detail(token, id).await?.summary.owner)
                    } else {
                        None
                    };
                    if owner.as_deref() == Some(&uid) {
                        return Err(Error::InvalidInput("cannot subscribe to own playlist"));
                    }
                }
                Ok(serde_json::to_value(list)?)
            }
            _ => {
                let detail = self
                    .detail(token, command.playlist_id().expect("detail command"))
                    .await?;
                detail.writable(&uid)?;
                if command.revision() != Some(&detail.revision) {
                    return Err(Error::QueueChanged);
                }
                if let PlaylistCommand::Tracks { change, ids, .. } = command {
                    if detail.has_local {
                        return Err(Error::State(
                            "local-only playlist entries require native matching before editing",
                        ));
                    }
                    if *change == TrackChange::Reorder {
                        operation::permutation(&detail.tracks, ids)?;
                    }
                }
                Ok(serde_json::to_value(detail)?)
            }
        }
    }
}
