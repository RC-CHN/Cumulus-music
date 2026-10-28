//! Durable local player application, separate from the remote pull receipt.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    membership::Quality,
    playback::PlaybackQueue,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
const JOURNAL: &str = "relay-playback-intents-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayPlaybackPhase {
    Preparing,
    Applying,
    Installed,
    Failed,
    Uncertain,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayPlaybackIntent {
    pub id: String,
    pub pull_id: String,
    pub previous_id: Option<String>,
    pub phase: RelayPlaybackPhase,
    pub created_at: u64,
    #[serde(default)]
    pub(crate) request: Option<RelayPlaybackRequest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RelayPlaybackRequest {
    pub revision: Option<String>,
    pub quality: Quality,
}
pub(crate) struct RelayPlayback {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl RelayPlayback {
    pub(crate) fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    pub(crate) fn lock(&self, token: &SessionToken) -> Result<(String, OperationLock)> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let lock = OperationLock::acquire(self.store.directory(), JOURNAL, &owner)?;
        Ok((owner, lock))
    }
    pub(crate) fn records(&self, token: &SessionToken) -> Result<Vec<RelayPlaybackIntent>> {
        self.session.with_token(token, |uid| {
            self.store
                .values(uid, JOURNAL)?
                .into_iter()
                .map(|(id, value)| {
                    let record: RelayPlaybackIntent = serde_json::from_value(value)?;
                    if record.id != id {
                        return Err(Error::State("relay application identity mismatch"));
                    }
                    validate_id(&record.id)?;
                    validate_id(&record.pull_id)?;
                    Ok(record)
                })
                .collect()
        })
    }
    pub(crate) fn record(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<RelayPlaybackIntent>> {
        validate_id(id)?;
        Ok(self.records(token)?.into_iter().find(|r| r.id == id))
    }
    /// Call with the account operation lock held across the entire prepare/apply.
    /// Existing IDs are inspections only: never replay an installed/unknown job.
    pub(crate) fn begin(
        &self,
        token: &SessionToken,
        id: &str,
        pull_id: &str,
        previous: Option<&str>,
        request: RelayPlaybackRequest,
    ) -> Result<Option<RelayPlaybackIntent>> {
        validate_id(id)?;
        validate_id(pull_id)?;
        if let Some(record) = self.record(token, id)? {
            if record.phase == RelayPlaybackPhase::Cancelled {
                return Ok(None);
            }
            if record.pull_id != pull_id || record.previous_id.as_deref() != previous {
                return Err(Error::InvalidInput("relay application intent reused"));
            }
            if record.request.as_ref().is_some_and(|old| old != &request) {
                return Err(Error::InvalidInput("relay application request changed"));
            }
            return Ok(None);
        }
        if let Some(previous) = previous {
            let old = self
                .record(token, previous)?
                .ok_or(Error::State("relay retry has no earlier intent"))?;
            if old.pull_id != pull_id
                || !matches!(
                    old.phase,
                    RelayPlaybackPhase::Failed | RelayPlaybackPhase::Uncertain
                )
            {
                return Err(Error::State("relay intent cannot be retried"));
            }
            if self
                .records(token)?
                .iter()
                .any(|r| r.previous_id.as_deref() == Some(previous))
            {
                return Err(Error::State("relay intent already has a retry"));
            }
        }
        let record = RelayPlaybackIntent {
            id: id.into(),
            pull_id: pull_id.into(),
            previous_id: previous.map(String::from),
            phase: RelayPlaybackPhase::Preparing,
            created_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            request: Some(request),
        };
        self.session
            .with_token(token, |owner| self.save_original(owner, &record))?;
        Ok(Some(record))
    }
    pub(crate) fn save_original(&self, owner: &str, record: &RelayPlaybackIntent) -> Result<()> {
        self.store
            .put(owner, JOURNAL, &record.id, &serde_json::to_value(record)?)
    }
    pub(crate) fn cancel_unseen(&self, token: &SessionToken, id: &str) -> Result<()> {
        validate_id(id)?;
        if self.record(token, id)?.is_some() {
            return Ok(());
        }
        let record = RelayPlaybackIntent {
            id: id.into(),
            pull_id: id.into(),
            previous_id: None,
            phase: RelayPlaybackPhase::Cancelled,
            request: None,
            created_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
        };
        self.session
            .with_token(token, |owner| self.save_original(owner, &record))
    }
    /// Only while no application worker owns the operation lock. A persisted
    /// current-entry marker proves queue installation, never audible playback.
    pub(crate) fn recover(&self, token: &SessionToken) -> Result<()> {
        self.session.with_token(token, |owner| {
            let queue = PlaybackQueue::new(Some(self.store.clone()), owner)?.snapshot();
            let installed = queue
                .items
                .iter()
                .find(|entry| Some(&entry.entry_id) == queue.current.as_ref())
                .and_then(|entry| entry.source["relay_application"].as_str());
            for (_, value) in self.store.values(owner, JOURNAL)? {
                let mut record: RelayPlaybackIntent = serde_json::from_value(value)?;
                if matches!(
                    record.phase,
                    RelayPlaybackPhase::Preparing | RelayPlaybackPhase::Applying
                ) {
                    record.phase = if installed == Some(record.id.as_str()) {
                        RelayPlaybackPhase::Installed
                    } else {
                        RelayPlaybackPhase::Uncertain
                    };
                    self.save_original(owner, &record)?;
                }
            }
            Ok(())
        })
    }
}
pub(crate) fn validate_id(id: &str) -> Result<()> {
    if id.len() != 36 || uuid::Uuid::parse_str(id).is_err() {
        return Err(Error::InvalidInput("invalid relay application ID"));
    }
    Ok(())
}
