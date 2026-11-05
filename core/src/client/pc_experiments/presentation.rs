//! M147's recoverable web presentation state, separate from live TTL grants.
use super::{MusicClient, ScopeContext, store};
use crate::{
    Error, Result,
    account::{SessionStatus, SessionToken},
    protocol::{api::Api, pc_experiments::PcExperimentGroups},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Flags {
    pub(super) search_v3: bool,
    pub(super) search_footer: bool,
    pub(super) search_expanded: bool,
    pub(super) search_dense: bool,
}

impl Flags {
    pub(super) fn project(groups: &PcExperimentGroups) -> Self {
        Self {
            search_v3: groups.group("PH-PC-searchHot") == Some("t1"),
            search_footer: groups.group("PH-PC-search-result") == Some("t1"),
            search_expanded: groups.group("PH-PC-searchMore2") == Some("t1"),
            search_dense: groups.group("PH-PC-searchPageEfficiency") == Some("t1"),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PcPresentationView {
    pub epoch: String,
    pub revision: String,
    pub search_footer: bool,
    pub search_expanded: bool,
    pub search_dense: bool,
}

impl PcPresentationView {
    fn from_flags(token: &SessionToken, revision: String, flags: Flags) -> Self {
        Self {
            epoch: token.generation.to_string(),
            revision,
            search_footer: flags.search_footer,
            search_expanded: flags.search_expanded,
            search_dense: flags.search_dense,
        }
    }
}

/// Internal endpoint choice is not a renderer-controlled experiment input.
#[derive(Clone)]
pub(in crate::client) struct PcPresentationSnapshot {
    pub(in crate::client) view: PcPresentationView,
    pub(in crate::client) search_v3: bool,
    owner: String,
}

struct Current {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    owner: String,
    flags: Flags,
    view: PcPresentationView,
}

impl Current {
    fn same_scope(&self, token: &SessionToken, api: &Arc<dyn Api>, owner: &str) -> bool {
        self.token == *token
            && self.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
            && self.owner == owner
    }

    fn matches(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &Option<String>,
        owner: &str,
    ) -> bool {
        self.same_scope(token, api, owner) && self.credentials == *credentials
    }
    fn snapshot(&self) -> PcPresentationSnapshot {
        PcPresentationSnapshot {
            view: self.view.clone(),
            search_v3: self.flags.search_v3,
            owner: self.owner.clone(),
        }
    }
}

pub(super) struct PresentationHost {
    state: Mutex<Option<Current>>,
    changes: watch::Sender<PcPresentationView>,
    pub(super) running: Arc<AtomicBool>,
    pub(super) cache_write_failed: AtomicBool,
    revision: AtomicU64,
}

impl Default for PresentationHost {
    fn default() -> Self {
        let view = PcPresentationView {
            epoch: "0".into(),
            revision: "0".into(),
            search_footer: false,
            search_expanded: false,
            search_dense: false,
        };
        Self {
            state: Mutex::new(None),
            changes: watch::channel(view).0,
            running: Arc::new(AtomicBool::new(false)),
            cache_write_failed: AtomicBool::new(false),
            revision: AtomicU64::new(0),
        }
    }
}

impl PresentationHost {
    fn next_revision(&self) -> Result<String> {
        self.revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|previous| (previous + 1).to_string())
            .map_err(|_| Error::State("PC presentation revision exhausted"))
    }
}

impl MusicClient {
    /// Pure native bootstrap/current snapshot. No request, timer or player lock.
    pub fn pc_presentation(&self) -> Result<PcPresentationView> {
        let token = self.session.token()?;
        let api = self.session.native_api(&token)?;
        self.pc_presentation_context()
            .snapshot(&token, &api)
            .map(|snapshot| snapshot.view)
    }

    pub fn subscribe_pc_presentation(&self) -> Result<watch::Receiver<PcPresentationView>> {
        self.pc_presentation()?;
        Ok(self.pc_experiments.presentation.changes.subscribe())
    }

    pub(in crate::client) fn pc_presentation_snapshot(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<super::PcPresentationSnapshot> {
        if api.pc_presentation_credential_revision()?.is_none() {
            return Err(Error::State(
                "PC presentation snapshot has no native transport",
            ));
        }
        self.pc_presentation_context().snapshot(token, api)
    }

    pub(in crate::client) fn pc_presentation_is_current(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        expected_revision: &str,
    ) -> Result<bool> {
        let snapshot = self.pc_presentation_snapshot(token, api)?;
        Ok(snapshot.view.revision == expected_revision)
    }
}

impl ScopeContext {
    pub(super) fn snapshot(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<PcPresentationSnapshot> {
        self.session.require_api(token, api)?;
        let credentials = api.pc_presentation_credential_revision().ok().flatten();
        let session = self.session.snapshot()?;
        if session.token != *token {
            return Err(Error::StaleResponse);
        }
        let domain = if session.status == SessionStatus::Authenticated && session.profile.is_some()
        {
            token.uid.clone()
        } else if token.uid.is_none() {
            match api.pc_anonymous_state() {
                Ok(crate::protocol::pc_experiments::PcAnonymousState::Registered {
                    uid,
                    revision,
                }) if credentials.as_deref() == Some(revision.as_str()) => {
                    Some(format!("anonymous:{uid}"))
                }
                _ => None,
            }
        } else {
            None
        };
        let owner = domain.clone().unwrap_or_else(|| "local-guest".into());
        let host = &self.host.presentation;
        let mut state = host.state.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(current) = state
            .as_ref()
            .filter(|value| value.matches(token, api, &credentials, &owner))
        {
            return Ok(current.snapshot());
        }
        let credential_changed = state.as_ref().is_some_and(|current| {
            current.same_scope(token, api, &owner) && current.credentials != credentials
        });
        let flags = if credential_changed {
            // Bootstrap is for a newly observed owner/context. Once this live
            // context has actually rotated credentials, reloading its old
            // display cache would resurrect a rejected previous-revision t1.
            // Only a new successful read may repopulate this context.
            Flags::default()
        } else if domain.is_some() && credentials.is_some() {
            let loaded = store::load(&self.store, &owner);
            match loaded {
                Ok(flags) => flags,
                Err(_) => {
                    host.cache_write_failed.store(true, Ordering::Release);
                    Flags::default()
                }
            }
        } else {
            // A blank MUSIC_U is not itself a successful anonymous receipt.
            Flags::default()
        };
        let view = PcPresentationView::from_flags(token, host.next_revision()?, flags);
        *state = Some(Current {
            token: token.clone(),
            api: Arc::downgrade(api),
            credentials,
            owner,
            flags,
            view: view.clone(),
        });
        host.changes.send_replace(view);
        Ok(state.as_ref().expect("presentation initialized").snapshot())
    }

    /// Only a successful live signed read reaches this hook. Persistence is a
    /// display bootstrap and is never copied back into `Assigned`/fresh grants.
    pub(super) fn commit_presentation(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        groups: &PcExperimentGroups,
    ) -> Result<()> {
        self.session.require_api(token, api)?;
        if api.pc_presentation_credential_revision()?.as_deref() != Some(revision) {
            return Err(Error::StaleResponse);
        }
        let snapshot = self.snapshot(token, api)?;
        let flags = Flags::project(groups);
        let host = &self.host.presentation;
        let persisted = store::save(&self.store, &snapshot.owner, flags);
        host.cache_write_failed
            .store(persisted.is_err(), Ordering::Release);
        self.session.require_api(token, api)?;
        if api.pc_presentation_credential_revision()?.as_deref() != Some(revision) {
            return Err(Error::StaleResponse);
        }
        let mut state = host.state.lock().map_err(|_| Error::LockPoisoned)?;
        let current = state
            .as_mut()
            .filter(|current| {
                current.matches(token, api, &Some(revision.to_owned()), &snapshot.owner)
                    && current.view.revision == snapshot.view.revision
            })
            .ok_or(Error::StaleResponse)?;
        if current.flags != flags {
            current.flags = flags;
            current.view = PcPresentationView::from_flags(token, host.next_revision()?, flags);
            host.changes.send_replace(current.view.clone());
        }
        Ok(())
    }
}
