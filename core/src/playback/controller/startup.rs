use super::{PlaybackStatus, StartupRestore, load::LoadOptions, state::Actor};
use crate::Result;

impl Actor {
    pub(super) fn recall_prefill(
        &mut self,
        mut input: super::RecallPrefill,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<()> {
        use crate::Error;
        if self.state != PlaybackStatus::Idle
            || self.loaded.is_some()
            || self.pending.is_some()
            || self.quality_task.is_some()
            || !self.queue.is_empty()
            || self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
        {
            return Err(Error::QueueChanged);
        }
        // Pending login inspects its own operation lock and must run before
        // the verified session lock. The latter fences actual adoption.
        (input.login_check)()?;
        self.stamp_quality(&mut input.resources);
        let session = self.session.clone();
        session.with_verified_profile(&self.token, &input.api, |owner, _| {
            let _privacy =
                crate::account::settings::AccountSettings::reserve_recall(&self.store, owner)?;
            if authority.is_cancelled() {
                return Err(Error::StaleResponse);
            }
            let now = input
                .api
                .observed_server_time_ms()
                .and_then(|value| i64::try_from(value).ok())
                .filter(|value| chrono::DateTime::from_timestamp_millis(*value).is_some())
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
            self.queue.install_recall(input.resources, now)
        })?;
        // A watch/Startup racing the first empty homepage must not open this
        // metadata-only selection. The next explicit Play uses the normal path.
        self.startup_pending = false;
        self.startup_autoplay = false;
        Ok(())
    }

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
        self.ordinary_chorus_trigger(super::ChorusTrigger::Startup);
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
