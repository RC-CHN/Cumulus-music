use super::{PlaybackStatus, state::Actor};
use crate::{Error, Result, playback::TimelineMode};

impl Actor {
    /// Failure stays separate from the audio result. Only the current loaded
    /// play can retry its local checkpoint, with the same original play time.
    pub(super) fn save_history(&mut self) {
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        if loaded.history.played_at.is_none() {
            if self.state != PlaybackStatus::Playing {
                return;
            }
            loaded.history.played_at = Some(
                (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64,
            );
        }
        let queue = self.queue.snapshot();
        let result: Result<()> = self.session.with_local_owner(&self.token, |owner| {
            self.store.transaction(|tx| {
                if self.queue.selected().as_ref() != Some(&loaded.item)
                    || tx.get(owner, "playback", "queue")? != Some(serde_json::to_value(queue)?)
                {
                    return Err(Error::QueueChanged);
                }
                crate::history::playback::save(
                    tx,
                    owner,
                    &loaded.item,
                    &loaded.history,
                    self.position.unwrap_or(0.0),
                    self.state == PlaybackStatus::Ended,
                    matches!(
                        loaded.timeline.snapshot().mode,
                        TimelineMode::Full | TimelineMode::FullTrial
                    ),
                )
            })
        });
        match result {
            Ok(()) => {
                loaded.history.committed = true;
                self.history_error = None;
            }
            Err(error) => {
                self.history_error = Some(error.to_string());
            }
        }
    }
}
