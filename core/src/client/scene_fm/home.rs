//! Native visit and transport authority for the verified classic podcast home.
use super::SceneFmHost;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::{
        MusicClient,
        dto::AccountData,
        focus::{FocusSnapshot, Owner},
    },
    playback::controller::{PlaybackFence, PlaybackStatus},
    podcast::scene::SceneFmService,
    protocol::api::Api,
};
use serde::Serialize;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

#[derive(Serialize)]
pub struct SceneFmHomeView {
    pub visit: String,
}

pub(in crate::client) struct ClassicObservation {
    token: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
}
impl ClassicObservation {
    pub(in crate::client) fn new(
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: String,
    ) -> Self {
        Self {
            token: token.clone(),
            api: Arc::downgrade(api),
            revision,
        }
    }
    fn check(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        if &self.token != token {
            return Err(Error::StaleResponse);
        }
        let api = self.api.upgrade().ok_or(Error::StaleResponse)?;
        session.with_verified_profile(token, &api, |_, _| Ok(()))?;
        if api.verified_pc_experiment_revision()?.as_deref() != Some(self.revision.as_str()) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

pub(super) struct InitialAudio {
    pub fence: PlaybackFence,
    pub selection: u64,
    pub focus: FocusSnapshot,
}

pub(super) struct HomeVisit {
    id: String,
    observation: Arc<ClassicObservation>,
    live: AtomicBool,
    pub audio: Option<InitialAudio>,
}
impl HomeVisit {
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
    }
    pub(super) fn check(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        if !self.live.load(Ordering::Acquire) {
            return Err(Error::StaleResponse);
        }
        self.observation.check(session, token)
    }
}

#[derive(Default)]
pub(super) struct HomeState {
    observation_sequence: u64,
    entry_sequence: u64,
    observation: Option<Arc<ClassicObservation>>,
    visit: Option<Arc<HomeVisit>>,
}
impl HomeState {
    fn retire(&mut self) {
        if let Some(visit) = self.visit.take() {
            visit.revoke();
        }
    }
}

impl SceneFmHost {
    pub(in crate::client) fn begin_top_observation(&self) -> Result<u64> {
        let mut state = self.home.lock().map_err(|_| Error::LockPoisoned)?;
        state.observation_sequence = state
            .observation_sequence
            .checked_add(1)
            .ok_or(Error::State("podcast top observation exhausted"))?;
        state.observation = None;
        state.retire();
        Ok(state.observation_sequence)
    }
    pub(in crate::client) fn finish_top_observation(
        &self,
        sequence: u64,
        classic: Option<ClassicObservation>,
    ) -> Result<bool> {
        let mut state = self.home.lock().map_err(|_| Error::LockPoisoned)?;
        if state.observation_sequence != sequence {
            return Err(Error::StaleResponse);
        }
        state.observation = classic.map(Arc::new);
        Ok(state.observation.is_some())
    }
    pub(super) fn home_visit(&self, token: &SessionToken, id: &str) -> Result<Arc<HomeVisit>> {
        self.home
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .visit
            .as_ref()
            .filter(|visit| visit.id == id && &visit.observation.token == token)
            .cloned()
            .ok_or(Error::StaleResponse)
    }
}

pub(super) fn idle_for_automatic(state: PlaybackStatus) -> bool {
    !matches!(
        state,
        PlaybackStatus::Playing
            | PlaybackStatus::Buffering
            | PlaybackStatus::Loading
            | PlaybackStatus::Preparing
    )
}

impl MusicClient {
    pub async fn scene_fm_home_enter(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Option<SceneFmHomeView>>> {
        let token = self.player_token(epoch)?;
        let mut value = AccountData {
            epoch: epoch.into(),
            value: None,
        };
        let (entry, observation) = {
            let mut state = self.scene_fm.home.lock().map_err(|_| Error::LockPoisoned)?;
            let Some(observation) = state.observation.clone() else {
                return Ok(value);
            };
            state.entry_sequence = state
                .entry_sequence
                .checked_add(1)
                .ok_or(Error::State("podcast home entry exhausted"))?;
            state.retire();
            (state.entry_sequence, observation)
        };
        observation.check(&self.session, &token)?;
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        // Capture the user intent *before* a directory request can start.
        // If autoplay is disabled, the visual entry needs no audio controller.
        let service = SceneFmService::new(self.session.clone())?;
        let focus = self.focus.snapshot()?;
        let selection = self.player.selection.load(Ordering::Acquire);
        let audio = if service.autoplay_for(&token)?
            && matches!(focus.owner, None | Some(Owner::Audio))
            && !self.has_together_cleanup()?
        {
            let controller = self.player.controller().await?;
            let (state, _) = controller.queue_with_state(&token).await?;
            (idle_for_automatic(state.state)
                && self.player.selection.load(Ordering::Acquire) == selection
                && self.focus.snapshot()? == focus)
                .then(|| InitialAudio {
                    fence: state.fence(),
                    selection,
                    focus,
                })
        } else {
            None
        };
        observation.check(&self.session, &token)?;
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let visit = Arc::new(HomeVisit {
            id: uuid::Uuid::new_v4().simple().to_string(),
            observation,
            live: AtomicBool::new(true),
            audio,
        });
        let mut state = self.scene_fm.home.lock().map_err(|_| Error::LockPoisoned)?;
        if state.entry_sequence != entry
            || !state
                .observation
                .as_ref()
                .is_some_and(|known| Arc::ptr_eq(known, &visit.observation))
        {
            return Err(Error::StaleResponse);
        }
        value.value = Some(SceneFmHomeView {
            visit: visit.id.clone(),
        });
        state.visit = Some(visit);
        Ok(value)
    }

    pub fn scene_fm_home_leave(&self, epoch: &str, visit: &str) -> Result<()> {
        let token = self.player_token(epoch)?;
        let mut state = self.scene_fm.home.lock().map_err(|_| Error::LockPoisoned)?;
        if state
            .visit
            .as_ref()
            .is_some_and(|current| current.id == visit && current.observation.token == token)
        {
            state.retire();
        }
        Ok(())
    }

    pub fn scene_fm_autoplay_preference(&self, epoch: &str) -> Result<AccountData<bool>> {
        let token = self.player_token(epoch)?;
        let service = SceneFmService::new(self.session.clone())?;
        let value = service.autoplay_for(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub fn set_scene_fm_autoplay_preference(
        &self,
        epoch: &str,
        enabled: bool,
    ) -> Result<AccountData<bool>> {
        let token = self.player_token(epoch)?;
        let service = SceneFmService::new(self.session.clone())?;
        service.set_autoplay_for(&token, enabled)?;
        let value = service.autoplay_for(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
