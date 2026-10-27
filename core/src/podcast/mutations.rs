//! M/232 mutations with durable intent and two matching reads (the recovered
//! probes observed eventual consistency). Reconciliation never replays writes.
use super::PodcastApi;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken, resources::MutationPhase},
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const JOURNAL: &str = "podcast-mutations-v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodcastAction {
    Subscribe,
    Like,
    Resume,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PodcastMutation {
    pub action: PodcastAction,
    pub id: String,
    pub before: bool,
    pub desired: bool,
    pub acknowledged: bool,
    pub observed: Option<bool>,
    pub phase: MutationPhase,
}
pub struct PodcastService {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    settle_delay: Duration,
}
impl PodcastService {
    fn save_transition(&self, owner: &str, key: &str, record: &PodcastMutation) -> Result<()> {
        self.store.transaction(|tx| {
            super::snapshot::invalidate(tx, owner)?;
            tx.put(owner, JOURNAL, key, &serde_json::to_value(record)?)
        })
    }
    pub fn operations_for(&self, token: &SessionToken) -> Result<Vec<PodcastMutation>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_values()
                .map(|v| serde_json::from_value(v).map_err(Into::into))
                .collect()
        })
    }
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            settle_delay: Duration::from_millis(500),
        })
    }
    pub fn with_settle_delay(mut self, delay: Duration) -> Self {
        self.settle_delay = delay;
        self
    }
    pub fn operation_for(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
    ) -> Result<Option<PodcastMutation>> {
        let key = key(action, id)?;
        self.session.with_token(token, |owner| {
            let record: Option<PodcastMutation> = self
                .store
                .get(owner, JOURNAL, &key)?
                .map(serde_json::from_value)
                .transpose()?;
            if record
                .as_ref()
                .is_some_and(|r| r.action != action || r.id != id)
            {
                return Err(Error::State("podcast mutation identity mismatch"));
            }
            Ok(record)
        })
    }
    pub async fn state_for(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
    ) -> Result<bool> {
        let id = identifier(&json!(id))?;
        self.session
            .run_for(token, |api, _| async move {
                let api = PodcastApi::new(api);
                match action {
                    PodcastAction::Subscribe => api
                        .radio(&json!(id))
                        .await?
                        .subscribed
                        .ok_or(Error::Response("podcast subscription state missing")),
                    PodcastAction::Like => Ok(api.liked_voices().await?.iter().any(|v| v.id == id)),
                    PodcastAction::Resume => api.resume_enabled(&json!(id)).await,
                }
            })
            .await
    }
    pub async fn change_for(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
        desired: bool,
    ) -> Result<PodcastMutation> {
        let key = key(action, id)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(self.store.directory(), &owner, &key)?;
        if self
            .operation_for(token, action, id)?
            .is_some_and(|v| v.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let before = self.state_for(token, action, id).await?;
        let mut record = PodcastMutation {
            action,
            id: id.into(),
            before,
            desired,
            acknowledged: false,
            observed: Some(before),
            phase: if before == desired {
                MutationPhase::Unchanged
            } else {
                MutationPhase::Sending
            },
        };
        self.session
            .with_token(token, |owner| self.save_transition(owner, &key, &record))?;
        if before == desired {
            return Ok(record);
        }
        let result = self
            .session
            .run_for(token, |api, _| async move {
                let api = PodcastApi::new(api);
                match action {
                    PodcastAction::Subscribe => api.subscribe(&json!(id), desired).await,
                    PodcastAction::Like => api.like(&json!(id), desired).await,
                    PodcastAction::Resume => api.set_resume_enabled(&json!(id), desired).await,
                }
            })
            .await;
        match result {
            Ok(()) => {
                record.acknowledged = true;
                record.phase = MutationPhase::Verifying;
            }
            Err(error) => {
                record.phase = if [301, 403, 405, 505, 506, 511, 512]
                    .iter()
                    .any(|code| error.is_service_code(*code))
                {
                    MutationPhase::Rejected
                } else {
                    MutationPhase::Uncertain
                };
                // Persist the original account's responsibility even after a
                // switch or a lost reply. Never publish it into the new account.
                self.save_transition(&owner, &key, &record)?;
                return Err(error);
            }
        }
        self.save_transition(&owner, &key, &record)?;
        self.settle(token, record).await
    }
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
    ) -> Result<PodcastMutation> {
        let key = key(action, id)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(self.store.directory(), &owner, &key)?;
        let record = self
            .operation_for(token, action, id)?
            .ok_or(Error::State("no podcast mutation to reconcile"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.settle(token, record).await
    }
    async fn settle(
        &self,
        token: &SessionToken,
        mut record: PodcastMutation,
    ) -> Result<PodcastMutation> {
        let key = key(record.action, &record.id)?;
        let mut matching = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(self.settle_delay).await;
            }
            let observed = self.state_for(token, record.action, &record.id).await?;
            matching = if observed == record.desired {
                matching + 1
            } else {
                0
            };
            record.observed = Some(observed);
            record.phase = if matching >= 2 {
                MutationPhase::Confirmed
            } else {
                MutationPhase::Uncertain
            };
            self.session
                .with_token(token, |owner| self.save_transition(owner, &key, &record))?;
            if matching >= 2 {
                return Ok(record);
            }
        }
        Err(Error::MutationUncertain)
    }
}
fn key(action: PodcastAction, id: &str) -> Result<String> {
    Ok(format!(
        "podcast:{}:{}",
        match action {
            PodcastAction::Subscribe => "subscribe",
            PodcastAction::Like => "like",
            PodcastAction::Resume => "resume",
        },
        identifier(&Value::String(id.into()))?
    ))
}
