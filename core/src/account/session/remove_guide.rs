//! Live M198/M1384 classic removeGuide. This is never persisted with the
//! explanatory LoginNotice and cannot survive a native account generation.
use super::{AccountSession, SessionStatus, SessionToken};
use crate::{Error, Result};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

const DELAY: Duration = Duration::from_secs(1);
const TTL: Duration = Duration::from_secs(300);

pub(super) struct ClassicRemoveGuide {
    generation: u64,
    ticket: String,
    issued: Instant,
    started: bool,
}

impl ClassicRemoveGuide {
    pub(super) fn new(generation: u64) -> Self {
        Self {
            generation,
            ticket: Uuid::new_v4().simple().to_string(),
            issued: Instant::now(),
            started: false,
        }
    }

    fn current(&self, generation: u64) -> bool {
        self.generation == generation && self.issued.elapsed() < TTL
    }

    pub(super) fn offer(&self, generation: u64) -> Option<String> {
        (self.current(generation) && !self.started).then(|| self.ticket.clone())
    }
}

impl AccountSession {
    pub(crate) fn begin_classic_remove_guide(
        &self,
        epoch: &str,
        ticket: &str,
        retry: bool,
    ) -> Result<SessionToken> {
        let mut state = self.lock()?;
        let token = state.token();
        if state.status != SessionStatus::Expired || token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let guide = state
            .classic_remove_guide
            .as_mut()
            .ok_or(Error::StaleResponse)?;
        if !guide.current(token.generation)
            || guide.ticket != ticket
            || guide.issued.elapsed() < DELAY
            || guide.started != retry
        {
            return Err(Error::StaleResponse);
        }
        guide.started = true;
        Ok(token)
    }

    pub(crate) fn require_classic_remove_guide(
        &self,
        token: &SessionToken,
        ticket: &str,
    ) -> Result<()> {
        let state = self.lock()?;
        state.require(token)?;
        if state.status != SessionStatus::Expired
            || !state.classic_remove_guide.as_ref().is_some_and(|guide| {
                guide.started && guide.current(token.generation) && guide.ticket == ticket
            })
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(crate) fn dismiss_classic_remove_guide(&self, epoch: &str, ticket: &str) -> Result<()> {
        let mut state = self.lock()?;
        if state.generation.to_string() == epoch
            && state
                .classic_remove_guide
                .as_ref()
                .is_some_and(|guide| guide.ticket == ticket)
        {
            state.classic_remove_guide = None;
        }
        Ok(())
    }
}
