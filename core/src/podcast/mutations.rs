//! M/232 mutations with durable intent and two matching reads (the recovered
//! probes observed eventual consistency). Reconciliation never replays writes.
use super::{
    PodcastApi,
    resume::{self, PlayerBinding},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken, resources::MutationPhase},
    playback::engine::Cancellation,
    protocol::api::{
        PODCAST_RESUME_WRITE_PATH, PlayerPodcastResumeWrite, VinylPlaybackReadPermission,
        VinylPodcastSubscriptionRead, VinylPodcastSubscriptionWrite, identifier, validate_response,
    },
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

#[derive(Clone, Copy)]
pub(crate) struct FreshVinylSubscription<'a> {
    pub revision: &'a str,
    pub permission: &'a VinylPlaybackReadPermission<'a>,
}

enum ChangeScope<'a> {
    Ordinary,
    Search,
    Context(&'a (dyn Fn() -> Result<()> + Sync + 'a)),
    Resume(PlayerBinding<'a>),
    Fresh(FreshVinylSubscription<'a>),
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
        if action == PodcastAction::Resume {
            return super::resume::read_for(&self.session, token, &json!(id)).await;
        }
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
                    PodcastAction::Resume => unreachable!("handled above"),
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
        self.change_checked(token, action, id, desired, ChangeScope::Ordinary)
            .await
    }
    pub async fn change_search_subscription_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected: bool,
    ) -> Result<PodcastMutation> {
        self.change_checked(
            token,
            PodcastAction::Subscribe,
            id,
            !expected,
            ChangeScope::Search,
        )
        .await
    }
    pub(crate) async fn change_vinyl_subscription_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected: bool,
        current: &(dyn Fn() -> Result<()> + Sync),
    ) -> Result<PodcastMutation> {
        self.change_checked(
            token,
            PodcastAction::Subscribe,
            id,
            !expected,
            ChangeScope::Context(current),
        )
        .await
    }
    pub(crate) async fn change_fresh_vinyl_subscription_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected: bool,
        binding: FreshVinylSubscription<'_>,
    ) -> Result<PodcastMutation> {
        self.change_checked(
            token,
            PodcastAction::Subscribe,
            id,
            !expected,
            ChangeScope::Fresh(binding),
        )
        .await
    }

    pub(crate) async fn change_player_resume_for(
        &self,
        token: &SessionToken,
        id: &str,
        desired: bool,
        revision: &str,
        permission: &VinylPlaybackReadPermission<'_>,
    ) -> Result<PodcastMutation> {
        self.change_checked(
            token,
            PodcastAction::Resume,
            id,
            desired,
            ChangeScope::Resume(PlayerBinding {
                revision,
                permission,
            }),
        )
        .await
    }

    async fn fresh_radio(
        &self,
        token: &SessionToken,
        id: &str,
        binding: FreshVinylSubscription<'_>,
    ) -> Result<super::Radio> {
        (binding.permission)()?;
        let read = VinylPodcastSubscriptionRead::new(id)?;
        let result = self
            .session
            .run_for(token, |api, _| async move {
                let reply = api
                    .request_vinyl_podcast_subscription_state(
                        read,
                        binding.revision,
                        binding.permission,
                    )
                    .await?;
                let reply = validate_response(VinylPodcastSubscriptionRead::PATH, reply, &[200])?;
                PodcastApi::radio_from_reply(id, &reply)
            })
            .await?;
        (binding.permission)()?;
        Ok(result)
    }

    async fn send_fresh_subscription(
        &self,
        token: &SessionToken,
        id: &str,
        desired: bool,
        binding: FreshVinylSubscription<'_>,
    ) -> Result<()> {
        // The typed transport marks the actual dispatch boundary. Once a
        // response arrives, keep its outcome in the original owner's journal
        // even if the now-visible account or episode no longer accepts it.
        let before_send = |error| Error::RequestNotSent(Box::new(error));
        (binding.permission)().map_err(before_send)?;
        let api = self.session.native_api(token).map_err(before_send)?;
        let write = VinylPodcastSubscriptionWrite::new(id, desired).map_err(before_send)?;
        let path = write.path();
        let reply = api
            .request_vinyl_podcast_subscription_write(write, binding.revision, binding.permission)
            .await?;
        validate_response(path, reply, &[200])?;
        Ok(())
    }
    pub(crate) async fn collect_recommendation_for(
        &self,
        token: &SessionToken,
        id: &str,
        source: &Cancellation,
    ) -> Result<PodcastMutation> {
        self.change_checked(
            token,
            PodcastAction::Subscribe,
            id,
            true,
            ChangeScope::Context(&|| source.check()),
        )
        .await
    }
    async fn change_checked(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
        desired: bool,
        scope: ChangeScope<'_>,
    ) -> Result<PodcastMutation> {
        let source: Option<&(dyn Fn() -> Result<()> + Sync)> = match scope {
            ChangeScope::Context(check) => Some(check),
            ChangeScope::Fresh(binding) => Some(binding.permission),
            ChangeScope::Resume(binding) => Some(binding.permission),
            _ => None,
        };
        let fresh = match scope {
            ChangeScope::Fresh(binding) => Some(binding),
            _ => None,
        };
        let key = key(action, id)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(self.store.directory(), &owner, &key)?;
        if let Some(source) = source {
            source()?;
        }
        if self
            .operation_for(token, action, id)?
            .is_some_and(|v| v.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let before = if let Some(binding) = fresh {
            let radio = self.fresh_radio(token, id, binding).await?;
            checked_subscription(radio, &owner)?
        } else if let ChangeScope::Resume(binding) = scope {
            resume::read_player_for(&self.session, token, id, binding).await?
        } else if matches!(scope, ChangeScope::Search | ChangeScope::Context(_)) {
            // The displayed result and membership list may have changed.
            // Check identity/ownership inside the same operation lock as the write.
            self.session
                .run_for(token, |api, uid| async move {
                    let radio = PodcastApi::new(api).radio(&json!(id)).await?;
                    checked_subscription(radio, &uid)
                })
                .await?
        } else {
            self.state_for(token, action, id).await?
        };
        if let Some(source) = source {
            source()?;
        }
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
        let result = if let Some(binding) = fresh {
            self.send_fresh_subscription(token, id, desired, binding)
                .await
        } else if let ChangeScope::Resume(PlayerBinding {
            revision,
            permission,
        }) = scope
        {
            let before_send = |error| Error::RequestNotSent(Box::new(error));
            async {
                permission().map_err(before_send)?;
                let api = self.session.native_api(token).map_err(before_send)?;
                let write = PlayerPodcastResumeWrite::new(id, desired).map_err(before_send)?;
                let reply = api
                    .request_player_podcast_resume_write(write, revision, permission)
                    .await?;
                validate_response(PODCAST_RESUME_WRITE_PATH, reply, &[200])?;
                Ok(())
            }
            .await
        } else {
            self.session
                .run_for(token, |api, _| async move {
                    if let Some(source) = source {
                        source().map_err(|error| Error::RequestNotSent(Box::new(error)))?;
                    }
                    let api = PodcastApi::new(api);
                    match action {
                        PodcastAction::Subscribe => api.subscribe(&json!(id), desired).await,
                        PodcastAction::Like => api.like(&json!(id), desired).await,
                        PodcastAction::Resume => api.set_resume_enabled(&json!(id), desired).await,
                    }
                })
                .await
        };
        match result {
            Ok(()) => {
                record.acknowledged = true;
                record.phase = MutationPhase::Verifying;
            }
            Err(error) => {
                record.phase = if source.is_some() && matches!(&error, Error::RequestNotSent(_))
                    || [301, 403, 405, 505, 506, 511, 512]
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
        let resume = match scope {
            ChangeScope::Resume(binding) => Some(binding),
            _ => None,
        };
        self.settle(token, record, fresh, resume).await
    }
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
    ) -> Result<PodcastMutation> {
        self.reconcile_checked(token, action, id, None, None).await
    }

    pub(crate) async fn reconcile_fresh_vinyl_subscription_for(
        &self,
        token: &SessionToken,
        id: &str,
        binding: FreshVinylSubscription<'_>,
    ) -> Result<PodcastMutation> {
        self.reconcile_checked(token, PodcastAction::Subscribe, id, Some(binding), None)
            .await
    }

    pub(crate) async fn reconcile_player_resume_for(
        &self,
        token: &SessionToken,
        id: &str,
        binding: PlayerBinding<'_>,
    ) -> Result<PodcastMutation> {
        self.reconcile_checked(token, PodcastAction::Resume, id, None, Some(binding))
            .await
    }

    async fn reconcile_checked(
        &self,
        token: &SessionToken,
        action: PodcastAction,
        id: &str,
        fresh: Option<FreshVinylSubscription<'_>>,
        resume: Option<PlayerBinding<'_>>,
    ) -> Result<PodcastMutation> {
        let key = key(action, id)?;
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let _lock = OperationLock::acquire(self.store.directory(), &owner, &key)?;
        if let Some(binding) = fresh {
            (binding.permission)()?;
        }
        if let Some(binding) = resume {
            (binding.permission)()?;
        }
        let record = self
            .operation_for(token, action, id)?
            .ok_or(Error::State("no podcast mutation to reconcile"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.settle(token, record, fresh, resume).await
    }
    async fn settle(
        &self,
        token: &SessionToken,
        mut record: PodcastMutation,
        fresh: Option<FreshVinylSubscription<'_>>,
        resume: Option<PlayerBinding<'_>>,
    ) -> Result<PodcastMutation> {
        let key = key(record.action, &record.id)?;
        let mut matching = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(self.settle_delay).await;
            }
            let observed = if let Some(binding) = fresh {
                self.fresh_radio(token, &record.id, binding)
                    .await?
                    .subscribed
                    .ok_or(Error::Response("podcast subscription state missing"))?
            } else if let Some(binding) = resume {
                resume::read_player_for(&self.session, token, &record.id, binding).await?
            } else {
                self.state_for(token, record.action, &record.id).await?
            };
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

fn checked_subscription(radio: super::Radio, uid: &str) -> Result<bool> {
    if radio
        .search_creator_id()
        .is_none_or(|id| id == uid.trim_start_matches('0'))
    {
        return Err(Error::InvalidInput(
            "podcast search subscription unavailable",
        ));
    }
    radio
        .subscribed
        .ok_or(Error::Response("podcast subscription state missing"))
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
