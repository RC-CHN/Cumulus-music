//! Only the native signed PC branch of the recovered experiment assignment.

use super::MusicClient;
use crate::{
    Error, Result,
    account::{SessionStatus, SessionToken},
    protocol::{api::Api, pc_experiments::PcExperimentGroups},
};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

const REFRESH: Duration = Duration::from_secs(30 * 60);
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub(super) struct PcExperimentHost {
    state: Mutex<Option<Assigned>>,
}

struct Assigned {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    fetched: Instant,
    groups: PcExperimentGroups,
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

    pub(super) async fn verified_pc_experiment_groups_for(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<(String, PcExperimentGroups)> {
        let session = self.session.snapshot()?;
        if session.token != *token {
            return Err(Error::StaleResponse);
        }
        if session.status != SessionStatus::Authenticated || self.login_pending()? {
            return Err(Error::State(
                "PC experiment requires a current authenticated account",
            ));
        }
        let mut changes = self.session.subscribe()?;
        let mut cached = self.pc_experiments.state.lock().await;
        self.session.require_api(token, api)?;
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let credentials = api.verified_pc_experiment_revision()?.ok_or(Error::State(
            "PC experiment requires a native signed transport",
        ))?;
        if let Some(current) = cached.as_ref().filter(|current| {
            current.token == *token
                && current
                    .api
                    .upgrade()
                    .is_some_and(|old| Arc::ptr_eq(&old, api))
                && current.credentials == credentials
                && current.fetched.elapsed() < REFRESH
        }) {
            return self.session.with_verified_profile(token, api, |_, _| {
                Ok((credentials, current.groups.clone()))
            });
        }
        let refresh =
            tokio::time::timeout(TIMEOUT, api.verified_pc_experiment_groups(&credentials));
        let groups = tokio::select! {
            biased;
            _ = changes.changed() => return Err(Error::StaleResponse),
            result = refresh => result.map_err(|_| Error::Transport("PC experiment assignment timed out".into()))??,
        };
        if self.login_pending()?
            || api.verified_pc_experiment_revision()?.as_deref() != Some(credentials.as_str())
        {
            return Err(Error::StaleResponse);
        }
        self.session.with_verified_profile(token, api, |_, _| {
            let result = groups.clone();
            *cached = Some(Assigned {
                token: token.clone(),
                api: Arc::downgrade(api),
                credentials: credentials.clone(),
                fetched: Instant::now(),
                groups,
            });
            Ok((credentials, result))
        })
    }
}

fn assignment(groups: &PcExperimentGroups) -> OrdinarySkipAssignment {
    if groups.skip_songs_assigned() {
        OrdinarySkipAssignment::Enabled
    } else {
        OrdinarySkipAssignment::Control
    }
}
