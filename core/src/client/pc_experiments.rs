//! Only the native signed PC branch of the recovered experiment assignment.
mod anonymous;
mod presentation;
mod runtime;
mod store;
pub(in crate::client) use presentation::PcPresentationSnapshot;
pub use presentation::PcPresentationView;
pub use runtime::{PcPresentationRuntimeLease, PcPresentationRuntimeStatus};

use super::MusicClient;
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken},
    protocol::{api::Api, pc_experiments::PcExperimentGroups},
    storage::StateStore,
};
use std::{
    sync::{Arc, Mutex as Observed, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

const REFRESH: Duration = Duration::from_secs(30 * 60);
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub(super) struct PcExperimentHost {
    gate: Mutex<()>,
    state: Observed<Option<Assigned>>,
    presentation: presentation::PresentationHost,
}

#[derive(Clone)]
struct ScopeContext {
    session: Arc<AccountSession>,
    host: Arc<PcExperimentHost>,
    store: Arc<StateStore>,
}

struct Assigned {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    fetched: Instant,
    groups: PcExperimentGroups,
}

impl Assigned {
    fn same_identity(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: &str) -> bool {
        self.token == *token
            && self.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
            && self.credentials == credentials
    }

    fn matches(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: &str) -> bool {
        self.same_identity(token, api, credentials) && self.fetched.elapsed() < REFRESH
    }
}

impl PcExperimentHost {
    /// M147's search presentation catch retains the last successful global
    /// web assignment, including one refreshed by another native consumer.
    /// This passive snapshot may be expired: it is not a fresh permission and
    /// must never replace `matches` in an authorization/dispatch decision.
    pub(super) fn search_catch_groups(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
    ) -> Result<Option<PcExperimentGroups>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|current| current.same_identity(token, api, credentials))
            .map(|current| current.groups.clone()))
    }

    /// Passive coherence check for consumers with an optional-read backoff.
    /// This never extends the shared assignment's original refresh deadline.
    pub(super) fn observed_groups(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
    ) -> Result<Option<PcExperimentGroups>> {
        Ok(self
            .observed_groups_with_version(token, api, credentials)?
            .map(|(_, groups)| groups))
    }

    pub(super) fn observed_groups_with_version(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
    ) -> Result<Option<(Instant, PcExperimentGroups)>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|current| current.matches(token, api, credentials))
            .map(|current| (current.fetched, current.groups.clone())))
    }

    /// A playback actor can fence an already authorized operation without
    /// taking the network single-flight gate or issuing another request.
    pub(super) fn observed_group(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
        name: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|current| current.matches(token, api, credentials))
            .and_then(|current| current.groups.group(name))
            .map(str::to_owned))
    }
}

/// Verified experiment output is intentionally not serializable as an IPC input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinarySkipAssignment {
    Control,
    Enabled,
}

impl MusicClient {
    /// Native-only read for the recovered ordinary-playback branch. This
    /// function never changes playback or activates a desktop scheduler.
    pub async fn official_pc_ordinary_skip_assignment(
        &self,
        epoch: &str,
    ) -> Result<OrdinarySkipAssignment> {
        self.verified_pc_experiment_groups(epoch)
            .await
            .map(|groups| assignment(&groups))
    }

    /// The full signed PC groups are only exposed to native use cases.
    pub(super) async fn verified_pc_experiment_groups(
        &self,
        epoch: &str,
    ) -> Result<PcExperimentGroups> {
        let session = self.session.snapshot()?;
        let token = session.token;
        if epoch != token.generation.to_string() {
            return Err(Error::StaleResponse);
        }
        let api = self.session.native_api(&token)?;
        self.verified_pc_experiment_groups_for(&token, &api)
            .await
            .map(|(_, groups)| groups)
    }

    fn pc_presentation_context(&self) -> ScopeContext {
        ScopeContext {
            session: self.session.clone(),
            host: self.pc_experiments.clone(),
            store: self.registry.store(),
        }
    }

    pub(super) async fn verified_pc_experiment_groups_for(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<(String, PcExperimentGroups)> {
        self.pc_presentation_context()
            .live_pc_experiment_groups_for(token, api, false, &|| {
                self.login_pending().map(|pending| !pending)
            })
            .await
    }

    /// Native host refresh only; no renderer command forces a cohort change.
    pub async fn refresh_pc_presentation(&self) -> Result<PcPresentationView> {
        let token = self.session.token()?;
        let context = self.pc_presentation_context();
        let mut api = self.session.native_api(&token)?;
        if token.uid.is_none() {
            api = context.isolate_guest(&token, &api)?.unwrap_or(api);
            context
                .live_anonymous_groups_for(&token, &api, true)
                .await?;
        } else {
            context
                .live_pc_experiment_groups_for(&token, &api, true, &|| {
                    self.login_pending().map(|pending| !pending)
                })
                .await?;
        }
        self.pc_presentation()
    }
}

impl ScopeContext {
    async fn live_pc_experiment_groups_for(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        scheduled: bool,
        allowed: &(dyn Fn() -> Result<bool> + Send + Sync),
    ) -> Result<(String, PcExperimentGroups)> {
        let requested = Instant::now();
        let session = self.session.snapshot()?;
        if session.token != *token {
            return Err(Error::StaleResponse);
        }
        if session.status != SessionStatus::Authenticated || !allowed()? {
            return Err(Error::State(
                "PC experiment requires a current authenticated account",
            ));
        }
        let mut changes = self.session.subscribe()?;
        let _gate = self.host.gate.lock().await;
        self.session.require_api(token, api)?;
        if !allowed()? {
            return Err(Error::StaleResponse);
        }
        let credentials = api.verified_pc_experiment_revision()?.ok_or(Error::State(
            "PC experiment requires a native signed transport",
        ))?;
        let cached = self
            .host
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|current| {
                current.matches(token, api, &credentials)
                    && (!scheduled || current.fetched >= requested)
            })
            .map(|current| current.groups.clone());
        if let Some(groups) = cached {
            let result = self
                .session
                .with_verified_profile(token, api, |_, _| Ok((credentials, groups)))?;
            let _ = self.commit_presentation(token, api, &result.0, &result.1);
            return Ok(result);
        }
        let refresh =
            tokio::time::timeout(TIMEOUT, api.verified_pc_experiment_groups(&credentials));
        let groups = tokio::select! {
            biased;
            _ = changes.changed() => return Err(Error::StaleResponse),
            result = refresh => result.map_err(|_| Error::Transport("PC experiment assignment timed out".into()))??,
        };
        if !allowed()?
            || api.verified_pc_experiment_revision()?.as_deref() != Some(credentials.as_str())
        {
            return Err(Error::StaleResponse);
        }
        let result = self.session.with_verified_profile(token, api, |_, _| {
            let result = groups.clone();
            *self.host.state.lock().map_err(|_| Error::LockPoisoned)? = Some(Assigned {
                token: token.clone(),
                api: Arc::downgrade(api),
                credentials: credentials.clone(),
                fetched: Instant::now(),
                groups,
            });
            Ok((credentials, result))
        })?;
        let _ = self.commit_presentation(token, api, &result.0, &result.1);
        Ok(result)
    }
}

fn assignment(groups: &PcExperimentGroups) -> OrdinarySkipAssignment {
    if groups.skip_songs_assigned() {
        OrdinarySkipAssignment::Enabled
    } else {
        OrdinarySkipAssignment::Control
    }
}
