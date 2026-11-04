//! Explicit handoff after an optional decoder's bounded seek was declined.
use super::{Cancellation, EngineEvent, Error, LoadedMedia, MpvEngine, Received, Result};
use crate::download::offline::ColdOfflineFlacReader;
use std::time::{Duration, Instant};

const SEEK_WAIT: Duration = Duration::from_secs(3);
const CLOCK_TOLERANCE: f64 = 0.010;

impl MpvEngine {
    /// Resume authorized cold FLAC with the stock decoder, bypassing the
    /// optional in-process path. The caller must already have confirmed the
    /// previous sink stopped/destroyed and selected the intended full-track
    /// media position; this method never selects, reopens, or retries a file.
    /// The same held reader remains protected by its original authority.
    pub fn resume_cold_flac_native(
        &mut self,
        reader: ColdOfflineFlacReader,
        position: f64,
        paused: bool,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if !position.is_finite() || position < 0.0 {
            return Err(Error::InvalidInput("invalid cold FLAC resume position"));
        }
        if self.entry.is_some() || self.pinned_authority.is_some() {
            return Err(Error::State("cold FLAC fallback requires an idle sink"));
        }
        let authority = reader.authority();
        authority.validate()?;
        cancel.check()?;
        let source = authority.cancellation();
        let active_request = cancel.clone();
        let active_source = source.clone();
        let guard = Cancellation::new(move || {
            !active_request.is_cancelled() && !active_source.is_cancelled()
        });

        // A held reader can still be positioned where the previous bounded
        // decoder failed. The pinned registry rewinds this same payload. The
        // ordinary loader always stages it paused, so no prefix is audible.
        let media = self.load_pinned(Box::new(reader), "flac", source, &guard)?;
        let result = (|| {
            if media.codec.as_deref() != Some("flac")
                || media.duration.is_none_or(|end| position >= end)
            {
                return Err(Error::InvalidInput(
                    "cold FLAC position is outside the native track",
                ));
            }
            let deadline = Instant::now() + SEEK_WAIT;
            if position > 0.0 {
                self.command(
                    vec![
                        c"seek".to_owned(),
                        super::string(position.to_string())?,
                        c"absolute+exact".to_owned(),
                    ],
                    deadline,
                    &guard,
                )?;
            }
            self.confirm_cold_position(media.entry_id, position, deadline, &guard)?;
            authority.validate()?;
            guard.check()?;
            if !paused {
                self.control_with_cancel(&["set", "pause", "no"], &guard)?;
                authority.validate()?;
            }
            guard.check()?;
            Ok(media)
        })();
        if result.is_err() {
            // It was never published to the caller. Do not leave its paused
            // data or events around for a subsequent unrelated command.
            self.retire_pinned();
        }
        result
    }

    fn confirm_cold_position(
        &mut self,
        entry: i64,
        target: f64,
        deadline: Instant,
        guard: &Cancellation,
    ) -> Result<()> {
        loop {
            guard.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || self.entry != Some(entry) || self.events.iter().any(|event| {
                matches!(event, EngineEvent::Shutdown)
                    || matches!(event, EngineEvent::Ended { entry_id, .. } if *entry_id == entry)
            }) {
                return Err(Error::Playback {
                    operation: "cold FLAC position was not confirmed",
                    code: -1,
                });
            }
            let idle_seek = self.property(c"seeking").as_deref() == Some("no");
            let observed = self.number(c"time-pos");
            if idle_seek
                && observed.is_some_and(|position| (position - target).abs() <= CLOCK_TOLERANCE)
                && self.property(c"seeking").as_deref() == Some("no")
                && self.property(c"pause").as_deref() == Some("yes")
            {
                return guard.check();
            }
            if let Received::Event(event) = self
                .native_mut()?
                .event(remaining.as_secs_f64().min(0.02))?
            {
                self.retain(event)?;
            }
        }
    }
}
