//! Observe committed history settings without using the experiment renderer ABI.
use super::super::MusicClient;
use crate::history::HistorySyncState;
use tokio::sync::watch;

#[derive(Default)]
pub(in crate::client) struct HistorySyncHost {
    changes: watch::Sender<Option<(u64, HistorySyncState)>>,
}

impl HistorySyncHost {
    pub(in crate::client) fn publish(&self, epoch: u64, value: HistorySyncState) {
        self.changes.send_if_modified(|current| {
            if current.is_some_and(|(old_epoch, old)| {
                (old_epoch, old.revision) >= (epoch, value.revision)
            }) {
                return false;
            }
            *current = Some((epoch, value));
            true
        });
    }
}

impl MusicClient {
    pub fn subscribe_history_sync(&self) -> watch::Receiver<Option<(u64, HistorySyncState)>> {
        self.history_sync.changes.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_commits_cannot_displace_newer_observations() {
        let host = HistorySyncHost::default();
        let mut changes = host.changes.subscribe();
        let state = |revision, enabled| HistorySyncState {
            enabled,
            first_disable_confirmed: false,
            revision,
        };
        host.publish(7, state(20, true));
        host.publish(8, state(0, false));
        assert_eq!(*changes.borrow_and_update(), Some((8, state(0, false))));
        host.publish(7, state(21, true));
        assert!(!changes.has_changed().unwrap());
        host.publish(8, state(2, true));
        host.publish(8, state(1, false));
        assert_eq!(*changes.borrow_and_update(), Some((8, state(2, true))));
    }
}
