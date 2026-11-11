//! One native file receipt for the latest Song or Voice history menu.
use super::{HistoryRowMenuTarget, song_file_menu::SongFileCandidate};
use crate::{
    Error, Result, account::SessionToken, download::completed::DownloadFileCandidate,
    history::HistoryKind,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(180);

#[derive(Default)]
pub(in crate::client) struct HistoryRowFileMenuHost(Mutex<(u64, Option<Offer>)>);

pub(super) enum Candidate {
    Song(SongFileCandidate),
    Voice(Box<DownloadFileCandidate>),
}

impl Candidate {
    fn kind(&self) -> HistoryKind {
        match self {
            Self::Song(_) => HistoryKind::Song,
            Self::Voice(_) => HistoryKind::Voice,
        }
    }
}

struct Offer {
    receipt: String,
    issued: Instant,
    token: SessionToken,
    target: HistoryRowMenuTarget,
    file: Candidate,
}

impl HistoryRowFileMenuHost {
    pub(super) fn begin(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.0 = state
            .0
            .checked_add(1)
            .ok_or(Error::State("history row menu generation exhausted"))?;
        state.1 = None;
        Ok(state.0)
    }

    pub(super) fn publish(
        &self,
        sequence: u64,
        token: SessionToken,
        target: HistoryRowMenuTarget,
        candidate: Option<Candidate>,
    ) -> Result<Option<String>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.0 != sequence {
            return Err(Error::StaleResponse);
        }
        let Some(file) = candidate else {
            return Ok(None);
        };
        if file.kind() != target.history_kind {
            return Err(Error::QueueChanged);
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        state.1 = Some(Offer {
            receipt: receipt.clone(),
            issued: Instant::now(),
            token,
            target,
            file,
        });
        Ok(Some(receipt))
    }

    pub(super) fn take(
        &self,
        token: &SessionToken,
        target: &HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<Candidate> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.1.as_ref().is_some_and(|offer| {
            &offer.token == token
                && &offer.target == target
                && offer.receipt == receipt
                && offer.issued.elapsed() < LIFETIME
        }) {
            return Err(Error::QueueChanged);
        }
        state
            .1
            .take()
            .map(|offer| offer.file)
            .ok_or(Error::QueueChanged)
    }
}
