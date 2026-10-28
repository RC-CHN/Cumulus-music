use super::{PlaybackStatus, StartupRestore, load::LoadOptions, state::Actor};
use crate::Result;

impl Actor {
    pub(super) fn startup_restore(&mut self, input: StartupRestore) -> Result<Option<LoadOptions>> {
        let autoplay = self.startup_autoplay && input.allow_autoplay && input.settings.auto_play;
        // Consume before persistence, resolving or decoding. A lost response,
        // failed load or reconnection must never cause a second automatic play.
        self.startup_pending = false;
        self.startup_autoplay = false;
        if self.state != PlaybackStatus::Idle {
            return Ok(None);
        }
        if let Some(queue) = input.queue {
            self.queue.install(queue)?;
        }
        let Some(selected) = self.queue.selected() else {
            return Ok(None);
        };
        let saved = self.store.get(self.queue.owner(), "playback", "position")?;
        let matched = saved.as_ref().filter(|v| {
            input.settings.remember_position
                && v["entry_id"].as_str() == Some(&selected.entry_id)
                && v["state"] != "ended"
        });
        let position = matched
            .and_then(|v| v["seconds"].as_f64())
            .filter(|p| p.is_finite() && *p >= 0.0);
        Ok(Some(LoadOptions {
            paused: !autoplay,
            // Explicit zero suppresses the separate podcast bookmark fallback.
            // It is relative to the authorized CUE/trial start, never file zero.
            position: Some(position.unwrap_or(0.0)),
            relative: position.is_none() || matched.is_some_and(|v| v["timeline_version"] != 1),
            recover: true,
            restart_at_end: true,
            ..Default::default()
        }))
    }
}
