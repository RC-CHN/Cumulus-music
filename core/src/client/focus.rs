//! Native media ownership. A handoff disables unpausing until the previous
//! controller has acknowledged suspension; late loads cannot steal ownership.
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    mv::playback::VideoCommand,
    playback::{controller::PlaybackCommand, engine::*},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    Audio,
    Video,
    ColdOffline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FocusSnapshot {
    pub generation: u64,
    pub owner: Option<Owner>,
}
enum ExpectedFocus {
    Automatic(u64),
    Native(FocusSnapshot),
}
#[derive(Default)]
struct State {
    generation: u64,
    allowed: Option<Owner>,
}
#[derive(Default)]
pub(super) struct MediaFocus {
    state: Mutex<State>,
    handoff: tokio::sync::Mutex<()>,
    cold_allowed: AtomicU64,
}
impl MediaFocus {
    pub fn cold_is_current(&self, lease: u64) -> bool {
        self.cold_allowed.load(Ordering::Acquire) == lease
    }
    pub fn release_cold(&self, lease: u64) {
        if let Ok(mut state) = self.state.lock()
            && state.generation == lease
            && state.allowed == Some(Owner::ColdOffline)
        {
            self.cold_allowed.store(0, Ordering::Release);
            state.allowed = None;
        }
    }
    pub fn start_cold(&self, lease: u64, start: impl FnOnce() -> Result<()>) -> Result<()> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != lease || state.allowed != Some(Owner::ColdOffline) {
            return Err(Error::StaleResponse);
        }
        start()
    }

    pub fn snapshot(&self) -> Result<FocusSnapshot> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(FocusSnapshot {
            generation: state.generation,
            owner: state.allowed,
        })
    }
    pub fn check_snapshot(&self, expected: FocusSnapshot) -> Result<()> {
        if self.snapshot()? != expected {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    pub fn automatic_audio_generation(&self) -> Result<Option<u64>> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        Ok((matches!(state.allowed, None | Some(Owner::Audio))).then_some(state.generation))
    }
    pub fn check(&self, lease: u64, owner: Owner) -> Result<()> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != lease || state.allowed != Some(owner) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}
impl MusicClient {
    pub(super) async fn suspend_account_media(&self) -> Result<()> {
        let token = self.session.token()?;
        {
            let mut state = self.focus.state.lock().map_err(|_| Error::LockPoisoned)?;
            state.generation = state
                .generation
                .checked_add(1)
                .ok_or(Error::State("media focus generation exhausted"))?;
            self.focus.cold_allowed.store(0, Ordering::Release);
            state.allowed = None;
        }
        let _handoff = self.focus.handoff.lock().await;
        self.cold_offline_close().await?;
        self.player.selection.fetch_add(1, Ordering::AcqRel);
        self.mv.retire_stale(&token).await?;
        if let Some(controller) = self.player.controller.get() {
            controller.command(&token, PlaybackCommand::Suspend).await?;
        }
        if let Some(controller) = self.mv.existing(&token)? {
            controller.command(&token, VideoCommand::Suspend).await?;
        }
        self.session.check_generation(&token)
    }

    pub(super) async fn claim_media(&self, token: &SessionToken, owner: Owner) -> Result<u64> {
        self.claim_media_if(token, owner, None).await
    }
    pub(super) async fn claim_media_if(
        &self,
        token: &SessionToken,
        owner: Owner,
        expected: Option<u64>,
    ) -> Result<u64> {
        self.claim_media_checked(token, owner, expected.map(ExpectedFocus::Automatic))
            .await
    }
    pub(super) async fn claim_native_media(
        &self,
        token: &SessionToken,
        owner: Owner,
        expected: FocusSnapshot,
    ) -> Result<u64> {
        self.claim_media_checked(token, owner, Some(ExpectedFocus::Native(expected)))
            .await
    }
    async fn claim_media_checked(
        &self,
        token: &SessionToken,
        owner: Owner,
        expected: Option<ExpectedFocus>,
    ) -> Result<u64> {
        let (lease, ()) = self.reserve_media_claim(token, expected, || Ok(()))?;
        self.finish_media_claim(token, owner, lease).await
    }

    /// Reserve a prepared selection's intent at the same point as its focus
    /// handoff. The supplied closure is synchronous and must not lock focus.
    pub(super) fn reserve_prepared_audio<T>(
        &self,
        token: &SessionToken,
        expected: FocusSnapshot,
        reserve: impl FnOnce() -> Result<T>,
    ) -> Result<(u64, T)> {
        self.reserve_media_claim(token, Some(ExpectedFocus::Native(expected)), reserve)
    }

    fn reserve_media_claim<T>(
        &self,
        token: &SessionToken,
        expected: Option<ExpectedFocus>,
        reserve: impl FnOnce() -> Result<T>,
    ) -> Result<(u64, T)> {
        self.session.check_generation(token)?;
        let mut state = self.focus.state.lock().map_err(|_| Error::LockPoisoned)?;
        if expected.is_some_and(|expected| match expected {
            ExpectedFocus::Automatic(g) => {
                g != state.generation || !matches!(state.allowed, None | Some(Owner::Audio))
            }
            ExpectedFocus::Native(value) => {
                value.generation != state.generation || value.owner != state.allowed
            }
        }) {
            return Err(Error::StaleResponse);
        }
        let lease = state
            .generation
            .checked_add(1)
            .ok_or(Error::State("media focus generation exhausted"))?;
        let reservation = reserve()?;
        state.generation = lease;
        self.focus.cold_allowed.store(0, Ordering::Release);
        state.allowed = None;
        Ok((lease, reservation))
    }

    pub(super) async fn finish_media_claim(
        &self,
        token: &SessionToken,
        owner: Owner,
        lease: u64,
    ) -> Result<u64> {
        let _handoff = self.focus.handoff.lock().await;
        if self
            .focus
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .generation
            != lease
        {
            return Err(Error::StaleResponse);
        }
        // Account replacement waits for old MV output to stop, including when
        // a concurrent snapshot is constructing the next account's controller.
        self.mv.retire_stale(token).await?;
        if owner != Owner::ColdOffline {
            self.cold_offline.stop_engine().await?;
        }
        match owner {
            Owner::ColdOffline => {
                self.player.selection.fetch_add(1, Ordering::AcqRel);
                if let Some(controller) = self.player.controller.get() {
                    controller.command(token, PlaybackCommand::Suspend).await?;
                }
                if let Some(controller) = self.mv.existing(token)? {
                    controller.command(token, VideoCommand::Suspend).await?;
                }
            }
            Owner::Audio => {
                if let Some(controller) = self.mv.existing(token)? {
                    controller.command(token, VideoCommand::Suspend).await?;
                }
            }
            Owner::Video => {
                self.player.selection.fetch_add(1, Ordering::AcqRel);
                if let Some(controller) = self.player.controller.get() {
                    controller.command(token, PlaybackCommand::Suspend).await?;
                }
            }
        }
        self.session.check_generation(token)?;
        let mut state = self.focus.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != lease {
            return Err(Error::StaleResponse);
        }
        state.allowed = Some(owner);
        if owner == Owner::ColdOffline {
            self.focus.cold_allowed.store(lease, Ordering::Release);
        }
        Ok(lease)
    }
}

pub(super) struct Focused<E: ?Sized> {
    pub inner: Box<E>,
    pub focus: Arc<MediaFocus>,
    pub owner: Owner,
}
impl<E: AudioEngine + ?Sized> AudioEngine for Focused<E> {
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        self.inner.output_devices()
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        // Device preference does not grant media ownership or unpause output.
        self.inner.set_output_device(id)
    }
    fn apply_parametric_component(
        &mut self,
        plan: crate::effects::parametric::ParametricPlan,
    ) -> Result<bool> {
        self.inner.apply_parametric_component(plan)
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        self.inner.clear_parametric_component()
    }
    fn apply_equalizer(
        &mut self,
        value: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<bool> {
        self.inner.apply_equalizer(value)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        self.inner.clear_equalizer()
    }
    fn observe_equalizer(
        &mut self,
    ) -> Result<Option<crate::effects::equalizer::abi::EqualizerStats>> {
        self.inner.observe_equalizer()
    }
    fn apply_loudness(
        &mut self,
        value: crate::effects::loudness::LoudnessSettings,
    ) -> Result<bool> {
        self.inner.apply_loudness(value)
    }
    fn clear_loudness(&mut self) -> Result<()> {
        self.inner.clear_loudness()
    }
    fn observe_loudness(&mut self) -> Result<Option<crate::effects::loudness::abi::LoudnessStats>> {
        self.inner.observe_loudness()
    }
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        // AudioEngine loads paused. Only pause(false) may start presentation.
        self.inner.load(r, c)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        if paused {
            return self.inner.pause(true);
        }
        let state = self.focus.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.allowed != Some(self.owner) {
            return Err(Error::Cancelled);
        }
        // Serialize the actual native unpause with revoking ownership.
        self.inner.pause(false)
    }
    fn seek(&mut self, s: f64) -> Result<()> {
        self.inner.seek(s)
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        self.inner.volume(v)
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.inner.speed(v)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let state = self.focus.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.allowed != Some(self.owner) {
            self.inner.pause(true)?;
        }
        self.inner.poll()
    }
}
impl<E: VideoEngine + ?Sized> VideoEngine for Focused<E> {
    fn resize(&self, w: u32, h: u32) -> Result<()> {
        self.inner.resize(w, h)
    }
    fn frame_after(&self, s: u64) -> Result<Option<Arc<VideoFrame>>> {
        self.inner.frame_after(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        protocol::{
            api::{Api, ApiFuture},
            http::ApiOrigin,
        },
    };
    struct NoNetwork;
    impl Api for NoNetwork {
        fn request<'a>(
            &'a self,
            _: &'a str,
            _: serde_json::Value,
            _: bool,
        ) -> ApiFuture<'a, serde_json::Value> {
            Box::pin(async { panic!("focus must not request network") })
        }
    }
    #[tokio::test]
    async fn automatic_audio_claim_rejects_newer_video_and_same_generation_video_owner() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(
            Arc::new(NoNetwork),
            Some(registry.store()),
        ));
        let token = session.token().unwrap();
        let client = MusicClient::with_session(session, registry);
        let old = client.focus.automatic_audio_generation().unwrap().unwrap();
        let video = client.claim_media(&token, Owner::Video).await.unwrap();
        assert!(client.focus.automatic_audio_generation().unwrap().is_none());
        assert!(matches!(
            client.claim_media_if(&token, Owner::Audio, Some(old)).await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            client
                .claim_media_if(&token, Owner::Audio, Some(video))
                .await,
            Err(Error::StaleResponse)
        ));
        client.focus.check(video, Owner::Video).unwrap();
        // An explicit user selection can still hand ownership back to audio.
        let audio = client.claim_media(&token, Owner::Audio).await.unwrap();
        client.focus.check(audio, Owner::Audio).unwrap();
    }
}
