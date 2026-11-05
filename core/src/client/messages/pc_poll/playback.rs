//! A narrow playback lease from the same native M119 actor as the count poll.
use crate::{account::SessionToken, playback::engine::Cancellation};
use tokio::sync::watch;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Sample {
    pub account: u64,
    pub document: u64,
    pub allowed: bool,
}

pub(super) struct PlaybackConnectivity {
    pub browser: watch::Receiver<(u64, Option<bool>)>,
    pub samples: watch::Receiver<Sample>,
}

impl PlaybackConnectivity {
    pub fn authority(&self, token: &SessionToken) -> Option<Cancellation> {
        let browser = self.browser.clone();
        let mut samples = self.samples.clone();
        let sample = samples.borrow_and_update().clone();
        if !sample.allowed || sample.account != token.generation {
            return None;
        }
        let guard = Cancellation::new(move || {
            // A revoked document cannot borrow the next document's online
            // state. has_changed also rejects online -> offline -> online.
            if browser.has_changed().is_err() || samples.has_changed().unwrap_or(true) {
                return false;
            }
            let (document, initial) = *browser.borrow();
            document == sample.document && initial.is_some()
        });
        (!guard.is_cancelled()).then_some(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_document_and_online_aba_cannot_keep_a_playback_lease() {
        let token = SessionToken {
            generation: 7,
            uid: Some("1".into()),
        };
        let (document, browser) = watch::channel((1, Some(true)));
        let ready = Sample {
            account: 7,
            document: 1,
            allowed: true,
        };
        let (publish, samples) = watch::channel(ready.clone());
        let connectivity = PlaybackConnectivity { browser, samples };
        let old = connectivity.authority(&token).unwrap();
        publish.send_replace(Sample {
            allowed: false,
            ..ready.clone()
        });
        publish.send_replace(ready);
        assert!(old.is_cancelled());
        let new = connectivity.authority(&token).unwrap();
        document.send_replace((2, None));
        assert!(new.is_cancelled());
        assert!(connectivity.authority(&token).is_none());
    }
}
