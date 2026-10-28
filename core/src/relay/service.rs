use super::{PullPlan, RelayAlert, RelayApi, RelayConfig};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const NAMESPACE: &str = "relay-v1";
const JOURNAL: &str = "relay-pull-intents-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayAvailability {
    Unchecked,
    ExperimentExcluded,
    ServerDisabled,
    Available,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelaySnapshot {
    pub availability: RelayAvailability,
    pub config: RelayConfig,
    pub setting: Option<bool>,
    pub alert: Option<RelayAlert>,
    pub revision: String,
    pub fetched_at: u64,
}
impl Default for RelaySnapshot {
    fn default() -> Self {
        Self {
            availability: RelayAvailability::Unchecked,
            config: RelayConfig::default(),
            setting: None,
            alert: None,
            revision: String::new(),
            fetched_at: 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullPhase {
    Pending,
    Ready,
    Rejected,
    Uncertain,
    Cancelled,
}
/// A ready plan proves a pull response, NOT that the audio player applied it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullIntent {
    pub id: String,
    pub revision: String,
    pub phase: PullPhase,
    pub plan: Option<PullPlan>,
}

pub struct RelayService {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
}
impl RelayService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    pub(super) fn lock(&self, token: &SessionToken) -> Result<(String, OperationLock)> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let lock = OperationLock::acquire(self.store.directory(), NAMESPACE, &owner)?;
        Ok((owner, lock))
    }
    pub fn snapshot_for(&self, token: &SessionToken) -> Result<RelaySnapshot> {
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, NAMESPACE, "snapshot")?
                .map(serde_json::from_value)
                .transpose()
                .map(|value| value.unwrap_or_default())
                .map_err(Into::into)
        })
    }
    fn save(&self, token: &SessionToken, snapshot: &mut RelaySnapshot) -> Result<()> {
        snapshot.revision = hex::encode(Sha256::digest(serde_json::to_vec(&(
            snapshot.availability,
            &snapshot.config,
            snapshot.setting,
            &snapshot.alert,
            snapshot.fetched_at,
        ))?));
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, NAMESPACE, "snapshot", &serde_json::to_value(snapshot)?)
        })
    }
    /// Refresh never reuses a cached allow flag after an interrupted/failed read.
    /// The UI may display old content separately but cannot use it to authorize.
    pub async fn refresh_for(&self, token: &SessionToken) -> Result<RelaySnapshot> {
        let (_owner, _lock) = self.lock(token)?;
        self.refresh_locked(token).await
    }
    async fn refresh_locked(&self, token: &SessionToken) -> Result<RelaySnapshot> {
        let mut snapshot = RelaySnapshot::default();
        self.save(token, &mut snapshot)?;
        let allowed = match self
            .session
            .run_for(token, |api, _| async move {
                RelayApi::new(api).experiment().await
            })
            .await
        {
            Ok(allowed) => allowed,
            Err(error) => {
                self.invalidate(token)?;
                return Err(error);
            }
        };
        if allowed {
            snapshot.config = self
                .session
                .run_for(
                    token,
                    |api, _| async move { RelayApi::new(api).config().await },
                )
                .await?;
            snapshot.availability = if snapshot.config.enable {
                RelayAvailability::Available
            } else {
                RelayAvailability::ServerDisabled
            };
        } else {
            snapshot.availability = RelayAvailability::ExperimentExcluded;
        }
        // The official settings view queries its switch independently of config.
        snapshot.setting = self
            .session
            .run_for(
                token,
                |api, _| async move { RelayApi::new(api).setting().await },
            )
            .await?;
        super::RelaySettings::new(self.session.clone())?.cache_for(token, snapshot.setting)?;
        if snapshot.availability == RelayAvailability::Available {
            snapshot.alert = self
                .session
                .run_for(
                    token,
                    |api, _| async move { RelayApi::new(api).alert().await },
                )
                .await?;
        }
        snapshot.fetched_at =
            (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64;
        self.save(token, &mut snapshot)?;
        Ok(snapshot)
    }
    pub fn dismiss_for(&self, token: &SessionToken, revision: &str) -> Result<()> {
        let (_owner, _lock) = self.lock(token)?;
        let mut snapshot = self.snapshot_for(token)?;
        if snapshot.revision != revision {
            return Err(Error::StaleResponse);
        }
        snapshot.alert = None;
        self.save(token, &mut snapshot)
    }
    pub fn intent_for(&self, token: &SessionToken, id: &str) -> Result<Option<PullIntent>> {
        validate_id(id)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, JOURNAL, id)?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    /// One explicit continuation intent performs at most one pull. The alert is
    /// consumed BEFORE the request, as in M/401. Duplicate IPC and restarts read
    /// the journal; neither automatically consumes a new remote device state.
    pub async fn pull_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
    ) -> Result<PullIntent> {
        validate_id(id)?;
        let (owner, _lock) = self.lock(token)?;
        if let Some(intent) = self.intent_for(token, id)? {
            if intent.revision != revision {
                return Err(Error::InvalidInput("relay intent identity reused"));
            }
            return Ok(intent);
        }
        let mut snapshot = self.snapshot_for(token)?;
        if snapshot.revision != revision || snapshot.availability != RelayAvailability::Available {
            return Err(Error::StaleResponse);
        }
        let alert = snapshot
            .alert
            .take()
            .ok_or(Error::State("no relay continuation available"))?;
        // Eligibility is checked again; disk cache or a prior process never
        // grants authority to consume an alert. No forced t2 / enable override.
        let allowed = match self
            .session
            .run_for(token, |api, _| async move {
                RelayApi::new(api).experiment().await
            })
            .await
        {
            Ok(allowed) => allowed,
            Err(error) => {
                self.invalidate(token)?;
                return Err(error);
            }
        };
        if !allowed {
            self.invalidate(token)?;
            return Err(Error::State("relay experiment excluded"));
        }
        let config = match self
            .session
            .run_for(
                token,
                |api, _| async move { RelayApi::new(api).config().await },
            )
            .await
        {
            Ok(config) => config,
            Err(error) => {
                self.invalidate(token)?;
                return Err(error);
            }
        };
        if !config.enable {
            self.invalidate(token)?;
            return Err(Error::State("relay server disabled"));
        }
        let mut intent = PullIntent {
            id: id.to_owned(),
            revision: revision.to_owned(),
            phase: PullPhase::Pending,
            plan: None,
        };
        snapshot.revision = hex::encode(Sha256::digest(serde_json::to_vec(&(
            revision, id, "consumed",
        ))?));
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                tx.put(
                    uid,
                    NAMESPACE,
                    "snapshot",
                    &serde_json::to_value(&snapshot)?,
                )?;
                tx.put(uid, JOURNAL, id, &serde_json::to_value(&intent)?)
            })
        })?;
        let outcome = self
            .session
            .run_for(token, |api, _| async move {
                RelayApi::new(api).pull(&alert).await
            })
            .await;
        match &outcome {
            Ok(plan) => {
                intent.phase = PullPhase::Ready;
                intent.plan = Some(plan.clone());
            }
            Err(error) => {
                intent.phase = match error.root() {
                    Error::Service { code, .. }
                        if [301, 403, 404]
                            .iter()
                            .any(|expected| crate::error::code_matches(code, *expected)) =>
                    {
                        PullPhase::Rejected
                    }
                    _ => PullPhase::Uncertain,
                };
            }
        }
        // Commit the outcome to the ORIGINAL account even if it changed during
        // the request. Never disclose the old plan to the new account.
        self.store
            .put(&owner, JOURNAL, id, &serde_json::to_value(&intent)?)?;
        self.session.check_generation(token)?;
        Ok(intent)
    }
    pub(super) fn invalidate(&self, token: &SessionToken) -> Result<()> {
        self.save(token, &mut RelaySnapshot::default())
    }
    /// Resolve a lost host acknowledgement before retrying an intent. An unseen
    /// ID is durably cancelled so a delayed original invocation cannot run it.
    pub fn cancel_unseen_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
    ) -> Result<PullIntent> {
        validate_id(id)?;
        let (_owner, _lock) = self.lock(token)?;
        if let Some(intent) = self.intent_for(token, id)? {
            if intent.revision != revision {
                return Err(Error::InvalidInput("relay intent identity reused"));
            }
            return Ok(intent);
        }
        let intent = PullIntent {
            id: id.to_owned(),
            revision: revision.to_owned(),
            phase: PullPhase::Cancelled,
            plan: None,
        };
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, JOURNAL, id, &serde_json::to_value(&intent)?)
        })?;
        Ok(intent)
    }
}
fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_err() || id.len() != 36 {
        return Err(Error::InvalidInput("invalid relay intent identifier"));
    }
    Ok(())
}
