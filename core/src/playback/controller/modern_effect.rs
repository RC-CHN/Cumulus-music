//! Native-only capture of the exact loaded source, before any effect I/O.
use super::{Message, PlaybackController, state::Actor};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    effects::dsp::Format,
    playback::engine::{Cancellation, NativeModernEffectSource},
};
use std::sync::{Arc, Weak};
use tokio::sync::oneshot;

#[cfg(test)]
mod actor_test;
#[cfg(test)]
pub(super) use actor_test::TestModernOwner;

pub(crate) struct ModernEffectSource {
    native: NativeModernEffectSource,
    playback: Cancellation,
    account: Weak<AccountSession>,
    token: SessionToken,
}

impl ModernEffectSource {
    pub(crate) fn check(&self) -> Result<()> {
        self.playback.check()?;
        self.native.check()?;
        self.playback.check()
    }

    pub(crate) fn check_for(
        &self,
        account: &Arc<AccountSession>,
        token: &SessionToken,
    ) -> Result<()> {
        if &self.token != token || !Weak::ptr_eq(&self.account, &Arc::downgrade(account)) {
            return Err(Error::StaleResponse);
        }
        account.check_generation(token)?;
        self.check()
    }

    pub(crate) fn format(&self) -> Format {
        self.native.format()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        native: NativeModernEffectSource,
        account: &Arc<AccountSession>,
        token: &SessionToken,
    ) -> Self {
        Self {
            native,
            playback: Cancellation::default(),
            account: Arc::downgrade(account),
            token: token.clone(),
        }
    }
}

pub(super) struct Request {
    token: SessionToken,
    pub(super) reply: oneshot::Sender<Result<ModernEffectSource>>,
}

impl PlaybackController {
    pub(crate) async fn capture_modern_effect_source(
        &self,
        token: &SessionToken,
    ) -> Result<ModernEffectSource> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Message::InspectModernEffect(Request {
                token: token.clone(),
                reply,
            }))
            .map_err(|_| Error::Cancelled)?;
        result.await.map_err(|_| Error::Cancelled)?
    }
}

impl Actor {
    pub(super) async fn modern_effect_source(
        &self,
        request: &Request,
    ) -> Result<ModernEffectSource> {
        self.session.check_generation(&request.token)?;
        if request.token != self.token
            || !self.state.active()
            || self.pending.is_some()
            || self.native_seeking
            || !self.parametric.admits_local_equalizer()
        {
            return Err(Error::State("no current owned modern-effect source"));
        }
        let loaded = self
            .loaded
            .as_ref()
            .filter(|loaded| loaded.local_equalizer)
            .ok_or(Error::State("no current owned modern-effect source"))?;
        let engine = self.engine.clone();
        let playback = self.guard();
        let entry = loaded.media.entry_id;
        let account = Arc::downgrade(&self.session);
        let token = request.token.clone();
        tokio::task::spawn_blocking(move || {
            playback.check()?;
            let engine = engine.lock().map_err(|_| Error::LockPoisoned)?;
            let native = engine
                .modern_effect_owned_source(entry)?
                .ok_or(Error::State("no current owned modern-effect source"))?;
            let receipt = ModernEffectSource {
                native,
                playback,
                account,
                token,
            };
            receipt.check()?;
            Ok(receipt)
        })
        .await
        .map_err(|_| Error::State("audio worker failed"))?
    }
}
