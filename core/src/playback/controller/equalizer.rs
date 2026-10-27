use super::{PlaybackStatus, state::Actor};
use crate::{
    Error, Result,
    effects::equalizer::{EqualizerPreferences, EqualizerSettings, EqualizerStatus},
    playback::engine::Cancellation,
};

impl Actor {
    pub(super) async fn equalizer_sync(&mut self) -> Result<()> {
        match self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::read(&self.store, owner)
        }) {
            Ok(preferences) => {
                if !self.equalizer_initialized
                    || self.equalizer.damaged
                    || preferences != self.equalizer.preferences
                {
                    self.equalizer.preferences = preferences;
                    self.equalizer.status = EqualizerStatus::Pending;
                    self.equalizer_initialized = true;
                    self.equalizer.damaged = false;
                }
            }
            Err(Error::Json(_)) => {
                if !self.equalizer.damaged {
                    self.equalizer.preferences = Default::default();
                    self.equalizer.status = EqualizerStatus::Pending;
                    self.equalizer_initialized = true;
                    self.equalizer.damaged = true;
                }
            }
            Err(error) => return Err(error),
        }
        // A decoder can hold the engine mutex. Save while loading, apply before
        // first unpause in loaded(), instead of blocking the actor's cancel path.
        if self.equalizer.status == EqualizerStatus::Pending && self.pending.is_none() {
            self.equalizer_apply().await?;
        }
        Ok(())
    }
    pub(super) async fn equalizer_change(
        &mut self,
        expected: u64,
        settings: EqualizerSettings,
    ) -> Result<()> {
        let preferences = self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::replace(&self.store, owner, expected, settings)
        })?;
        self.equalizer.preferences = preferences;
        self.equalizer.status = EqualizerStatus::Pending;
        self.equalizer_initialized = true;
        if self.pending.is_none() {
            self.equalizer_apply().await?;
        }
        Ok(())
    }
    pub(super) async fn equalizer_retry(&mut self) -> Result<()> {
        self.equalizer.status = EqualizerStatus::Pending;
        self.equalizer_sync().await
    }
    pub(super) async fn equalizer_repair(&mut self) -> Result<()> {
        self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::repair(&self.store, owner)
        })?;
        self.equalizer_sync().await
    }
    pub(super) async fn equalizer_apply(&mut self) -> Result<()> {
        let settings = self.equalizer.preferences.settings.effective();
        let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
            match engine.apply_equalizer(settings) {
                Ok(available) => Ok(if available {
                    EqualizerStatus::Configured
                } else {
                    EqualizerStatus::Unavailable
                }),
                Err(_) => {
                    // An old curve must not silently remain active after a new
                    // durable setting failed. If cleanup fails, stop playback.
                    if let Err(error) = engine.clear_equalizer() {
                        let _ = engine.stop();
                        return Err(error);
                    }
                    Ok(EqualizerStatus::Failed)
                }
            }
        })
        .await;
        match result {
            Ok(status) => {
                self.equalizer.status = if self.equalizer.damaged {
                    EqualizerStatus::Failed
                } else {
                    status
                }
            }
            Err(error) => {
                self.equalizer.status = EqualizerStatus::Failed;
                if !matches!(error.root(), Error::Cancelled | Error::StaleResponse) {
                    self.state = PlaybackStatus::Error;
                    self.error = Some(error.to_string());
                }
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) async fn equalizer_clear(&mut self) -> Result<()> {
        self.equalizer = Default::default();
        self.equalizer_initialized = false;
        super::engine(self.engine.clone(), Cancellation::default(), |engine| {
            engine.clear_equalizer()
        })
        .await
    }
    pub(super) async fn equalizer_observe(&mut self) -> Result<()> {
        if !matches!(
            self.equalizer.status,
            EqualizerStatus::Configured | EqualizerStatus::Processing
        ) {
            return Ok(());
        }
        let result = match super::engine(self.engine.clone(), self.guard(), |engine| {
            engine.observe_equalizer()
        })
        .await
        {
            Ok(value) => value,
            Err(error) => {
                self.equalizer.status = EqualizerStatus::Failed;
                return Err(error);
            }
        };
        self.equalizer.status = match result {
            Some(stats) if stats.failed_blocks != 0 => EqualizerStatus::Failed,
            Some(stats)
                if stats.active_instances != 0
                    && stats.processed_samples != 0
                    && stats.observed_revision == stats.revision =>
            {
                EqualizerStatus::Processing
            }
            _ => EqualizerStatus::Configured,
        };
        Ok(())
    }
}
