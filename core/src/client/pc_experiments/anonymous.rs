//! Source-proven registered guest domain, never an authenticated session UID.
use super::*;
use crate::protocol::pc_experiments::PcAnonymousState;

impl ScopeContext {
    /// Pure native transport handoff before registration. No Cookie is copied
    /// from a selected account jar. Unknown waits for a definitive fresh 301.
    pub(super) fn isolate_guest(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<Option<Arc<dyn Api>>> {
        let current = self.session.snapshot()?;
        if current.token != *token
            || token.uid.is_some()
            || !self.session.guest_ready_for(token, api)?
        {
            return Err(Error::StaleResponse);
        }
        self.session.require_api(token, api)?;
        let directory = self.store.directory().join("guest");
        if api.pc_anonymous_store_matches(&directory)? {
            return Ok(None);
        }
        let isolated = api
            .isolated_pc_anonymous_transport(&directory)?
            .ok_or(Error::State("PC anonymous guest transport is unavailable"))?;
        self.session
            .replace_guest_api(token, api, isolated.clone())?;
        Ok(Some(isolated))
    }

    pub(super) async fn live_anonymous_groups_for(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        scheduled: bool,
    ) -> Result<(String, PcExperimentGroups)> {
        let before = self.session.snapshot()?;
        if before.token != *token
            || token.uid.is_some()
            || !self.session.guest_ready_for(token, api)?
        {
            return Err(Error::StaleResponse);
        }
        self.session.require_api(token, api)?;
        let mut changes = self.session.subscribe()?;
        let identity = match api.pc_anonymous_state()? {
            registered @ PcAnonymousState::Registered { .. } => registered,
            PcAnonymousState::Unregistered => tokio::select! {
                biased;
                _ = changes.changed() => return Err(Error::StaleResponse),
                result = tokio::time::timeout(Duration::from_secs(30), api.register_pc_anonymous()) =>
                    result.map_err(|_| Error::Transport("PC anonymous registration timed out".into()))??,
            },
            PcAnonymousState::Uncertain => {
                return Err(Error::State(
                    "PC anonymous registration acknowledgement is unknown",
                ));
            }
            PcAnonymousState::Unsupported => {
                return Err(Error::State(
                    "PC anonymous registration prerequisite is unavailable",
                ));
            }
        };
        let PcAnonymousState::Registered {
            uid,
            revision: credentials,
        } = identity
        else {
            return Err(Error::State(
                "PC anonymous registration acknowledgement is unknown",
            ));
        };
        self.session.require_api(token, api)?;
        self.snapshot(token, api)?;
        let requested = Instant::now();
        let _gate = self.host.gate.lock().await;
        self.session.require_api(token, api)?;
        let current_identity = || {
            matches!(api.pc_anonymous_state(), Ok(PcAnonymousState::Registered { uid: current_uid, revision })
                if current_uid == uid && revision == credentials)
        };
        if !current_identity() {
            return Err(Error::StaleResponse);
        }
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
        let (groups, live) = match cached {
            Some(groups) => (groups, false),
            None => tokio::select! {
                biased;
                _ = changes.changed() => return Err(Error::StaleResponse),
                result = tokio::time::timeout(TIMEOUT, api.pc_presentation_groups(&credentials)) =>
                    (result.map_err(|_| Error::Transport("PC experiment assignment timed out".into()))??, true),
            },
        };
        self.session.require_api(token, api)?;
        if !current_identity() {
            return Err(Error::StaleResponse);
        }
        if live {
            *self.host.state.lock().map_err(|_| Error::LockPoisoned)? = Some(Assigned {
                token: token.clone(),
                api: Arc::downgrade(api),
                credentials: credentials.clone(),
                fetched: Instant::now(),
                groups: groups.clone(),
            });
            self.commit_presentation(token, api, &credentials, &groups)?;
        }
        Ok((credentials, groups))
    }
}
