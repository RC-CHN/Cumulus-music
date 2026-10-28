mod dto;
mod library;
mod social;
mod video;
mod view;
use super::{
    MusicClient,
    focus::{Focused, MediaFocus, Owner},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    mv::playback::{VideoController, VideoEngineFactory},
    playback::engine::{MpvConfig, MpvVideoEngine},
};
pub use dto::*;
use std::sync::{Arc, Mutex};

pub(super) struct MvHost {
    session: Arc<AccountSession>,
    focus: Arc<MediaFocus>,
    factory: VideoEngineFactory,
    output: Arc<crate::playback::output::OutputRuntime>,
    current: Mutex<Option<(SessionToken, Arc<VideoController>)>>,
    construction: tokio::sync::Mutex<()>,
}
impl MvHost {
    pub fn new(
        session: Arc<AccountSession>,
        focus: Arc<MediaFocus>,
        output: Arc<crate::playback::output::OutputRuntime>,
    ) -> Self {
        Self {
            session,
            focus,
            output,
            factory: Arc::new(|| {
                Ok(Box::new(MpvVideoEngine::open(
                    MpvConfig::default(),
                    640,
                    360,
                )?))
            }),
            current: Mutex::new(None),
            construction: tokio::sync::Mutex::new(()),
        }
    }
    fn token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    pub fn existing(&self, token: &SessionToken) -> Result<Option<Arc<VideoController>>> {
        let current = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(current
            .as_ref()
            .filter(|(t, _)| t == token)
            .map(|(_, c)| c.clone()))
    }
    async fn retire_locked(&self, token: &SessionToken) -> Result<()> {
        self.session.check_generation(token)?;
        let old = {
            let current = self.current.lock().map_err(|_| Error::LockPoisoned)?;
            current
                .as_ref()
                .filter(|(t, _)| t != token)
                .map(|(_, c)| c.clone())
        };
        if let Some(old) = old {
            old.shutdown().await?;
            // Retain responsibility until the shutdown acknowledgement. If
            // this future is cancelled, the next caller waits for the same core.
            self.current.lock().map_err(|_| Error::LockPoisoned)?.take();
        }
        self.session.check_generation(token)
    }
    pub(super) async fn retire_stale(&self, token: &SessionToken) -> Result<()> {
        let _construction = self.construction.lock().await;
        self.retire_locked(token).await
    }
    async fn controller(&self, token: &SessionToken) -> Result<Arc<VideoController>> {
        let _construction = self.construction.lock().await;
        self.retire_locked(token).await?;
        if let Some(controller) = self.existing(token)? {
            return Ok(controller);
        }
        let factory = self.factory.clone();
        let focus = self.focus.clone();
        let output = self.output.clone();
        let controller = Arc::new(VideoController::start(
            self.session.clone(),
            token.clone(),
            Arc::new(move || {
                let lease = output.reserve(crate::playback::output::OutputKind::Video)?;
                Ok(Box::new(lease.wrap(Box::new(Focused {
                    inner: factory()?,
                    focus: focus.clone(),
                    owner: Owner::Video,
                }))))
            }),
        )?);
        self.session.with_token(token, |_| Ok(()))?;
        *self.current.lock().map_err(|_| Error::LockPoisoned)? =
            Some((token.clone(), controller.clone()));
        Ok(controller)
    }
}
impl MusicClient {
    /// Native host/test injection before any MV player operation.
    pub fn with_video_factory(mut self, factory: VideoEngineFactory) -> Self {
        self.mv.factory = factory;
        self
    }
}
