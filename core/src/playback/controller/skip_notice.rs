//! Ephemeral observation of an actor-committed ordinary copyright skip.
use super::{PlaybackController, state::Actor};
use crate::{Error, Result, account::SessionToken, playback::source::SourceResolver};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    time::Instant,
};

const TTL: Duration = Duration::from_secs(3);
const CAPACITY: usize = 16;

#[derive(Clone)]
pub(super) struct CommittedSkip {
    token: SessionToken,
    id: uuid::Uuid,
    at: Instant,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinarySkipNotice {
    pub epoch: String,
    pub id: String,
    pub remaining_ms: u32,
}

/// A native-only, non-replaying observation tied to one account generation.
pub struct OrdinarySkipNotices {
    token: SessionToken,
    resolver: Arc<SourceResolver>,
    events: broadcast::Receiver<CommittedSkip>,
}

pub(super) fn publisher() -> broadcast::Sender<CommittedSkip> {
    broadcast::channel(CAPACITY).0
}

impl PlaybackController {
    pub(crate) fn ordinary_skip_notices(&self, token: SessionToken) -> Result<OrdinarySkipNotices> {
        self.resolver.check_account_generation(&token)?;
        Ok(OrdinarySkipNotices {
            token,
            resolver: self.resolver.clone(),
            events: self.skip_notices.subscribe(),
        })
    }
}

impl OrdinarySkipNotices {
    pub async fn next(&mut self) -> Result<OrdinarySkipNotice> {
        loop {
            self.resolver.check_account_generation(&self.token)?;
            let event = match self.events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return Err(Error::Cancelled),
            };
            self.resolver.check_account_generation(&self.token)?;
            if event.token != self.token {
                continue;
            }
            let remaining_ms = TTL.saturating_sub(event.at.elapsed()).as_millis() as u32;
            if remaining_ms > 0 {
                return Ok(OrdinarySkipNotice {
                    epoch: self.token.generation.to_string(),
                    id: event.id.simple().to_string(),
                    remaining_ms,
                });
            }
        }
    }
}

impl Actor {
    pub(super) fn publish_ordinary_skip(&self) {
        if self.token.uid.is_some() && self.session.check_generation(&self.token).is_ok() {
            let _ = self.skip_notices.send(CommittedSkip {
                token: self.token.clone(),
                id: uuid::Uuid::new_v4(),
                at: Instant::now(),
            });
        }
    }
}
