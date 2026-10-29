use super::{PlaybackStatus, state::Actor};
use crate::{
    Error, Result,
    effects::loudness::{LoudnessPreferences, LoudnessSettings, LoudnessStatus, TrackGain},
    playback::source::ResolvedSource,
};

impl Actor {
    pub(super) fn loudness_forget_track(&mut self) {
        self.loudness.gain = None;
        self.loudness.invalid_gain = false;
        self.loudness.has_track = false;
        self.loudness.status = if self.loudness.damaged {
            LoudnessStatus::Failed
        } else if self.loudness.preferences.enabled {
            LoudnessStatus::NoTrack
        } else {
            LoudnessStatus::Off
        };
    }
    pub(super) async fn loudness_source(&mut self, source: &ResolvedSource) -> Result<()> {
        self.loudness.has_track = true;
        let raw = if source.info["kind"] == "online" {
            &source.info["resource_info"]["gain"]
        } else {
            &source.info["volume_delta"]
        };
        let gain = TrackGain::from_volume_delta(raw);
        self.loudness.invalid_gain = gain.is_err() || source.info["loudness_invalid"] == true;
        self.loudness.gain = gain.ok().flatten();
        if self
            .loudness
            .gain
            .is_some_and(|gain| LoudnessSettings::new(true, gain.decibels()).is_err())
        {
            self.loudness.invalid_gain = true;
            self.loudness.gain = None;
        }
        self.loudness.status = LoudnessStatus::Pending;
        // The setting may have changed in another window while decoding.
        self.loudness_sync().await
    }
    pub(super) async fn loudness_sync(&mut self) -> Result<()> {
        match self
            .session
            .with_local_owner(&self.token, |_| LoudnessPreferences::read(&self.store))
        {
            Ok(preferences) => {
                if !self.loudness_initialized
                    || self.loudness.damaged
                    || preferences != self.loudness.preferences
                {
                    self.loudness.preferences = preferences;
                    self.loudness.damaged = false;
                    self.loudness_initialized = true;
                    self.loudness.status = LoudnessStatus::Pending;
                }
            }
            Err(Error::Json(_)) => {
                if !self.loudness.damaged {
                    self.loudness.preferences = Default::default();
                    self.loudness.damaged = true;
                    self.loudness_initialized = true;
                    self.loudness.status = LoudnessStatus::Pending;
                }
            }
            Err(error) => return Err(error),
        }
        // Saving a preference must not wait on a decoder that holds the mutex.
        if self.pending.is_none() && self.loudness.status == LoudnessStatus::Pending {
            self.loudness_apply().await?;
        }
        Ok(())
    }
    pub(super) async fn loudness_change(&mut self, expected: &str, enabled: bool) -> Result<()> {
        self.session.with_local_owner(&self.token, |_| {
            LoudnessPreferences::replace(&self.store, expected, enabled)
        })?;
        self.loudness_sync().await
    }
    pub(super) async fn loudness_retry(&mut self) -> Result<()> {
        self.loudness.status = LoudnessStatus::Pending;
        self.loudness_sync().await
    }
    pub(super) async fn loudness_repair(&mut self) -> Result<()> {
        self.session
            .with_local_owner(&self.token, |_| LoudnessPreferences::repair(&self.store))?;
        self.loudness_sync().await
    }
    async fn loudness_apply(&mut self) -> Result<()> {
        let idle_status = if self.loudness.damaged {
            Some(LoudnessStatus::Failed)
        } else if !self.loudness.preferences.enabled {
            Some(LoudnessStatus::Off)
        } else if !self.loudness.has_track {
            Some(LoudnessStatus::NoTrack)
        } else if self.loudness.invalid_gain {
            Some(LoudnessStatus::InvalidGain)
        } else if self.loudness.gain.is_none() {
            Some(LoudnessStatus::NoGain)
        } else {
            None
        };
        let settings = LoudnessSettings::new(
            idle_status.is_none(),
            self.loudness.gain.map_or(0.0, TrackGain::decibels),
        );
        let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
            let apply = settings.and_then(|value| engine.apply_loudness(value));
            match apply {
                Ok(available) => Ok(idle_status.unwrap_or(if available {
                    LoudnessStatus::Configured
                } else {
                    LoudnessStatus::Unavailable
                })),
                Err(_) => {
                    if let Err(error) = engine.clear_loudness() {
                        let _ = engine.stop();
                        return Err(error);
                    }
                    Ok(LoudnessStatus::Failed)
                }
            }
        })
        .await;
        match result {
            Ok(status) => self.loudness.status = status,
            Err(error) => {
                self.loudness.status = LoudnessStatus::Failed;
                if !matches!(error.root(), Error::Cancelled | Error::StaleResponse) {
                    self.state = PlaybackStatus::Error;
                    self.error = Some(error.to_string());
                }
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) async fn loudness_observe(&mut self) -> Result<()> {
        if !matches!(
            self.loudness.status,
            LoudnessStatus::Configured | LoudnessStatus::Processing
        ) {
            return Ok(());
        }
        let result = super::engine(self.engine.clone(), self.guard(), |engine| {
            match engine.observe_loudness() {
                Ok(Some(stats)) if stats.failed_blocks == 0 => Ok(
                    if stats.active_instances != 0
                        && stats.processed_samples != 0
                        && stats.observed_revision == stats.revision
                    {
                        LoudnessStatus::Processing
                    } else {
                        LoudnessStatus::Configured
                    },
                ),
                Ok(None) => Ok(LoudnessStatus::Configured),
                _ => {
                    if let Err(error) = engine.clear_loudness() {
                        let _ = engine.stop();
                        return Err(error);
                    }
                    Ok(LoudnessStatus::Failed)
                }
            }
        })
        .await;
        self.loudness.status = result.as_ref().copied().unwrap_or(LoudnessStatus::Failed);
        result.map(|_| ())
    }
}
