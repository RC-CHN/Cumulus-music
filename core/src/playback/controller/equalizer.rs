use super::{PlaybackStatus, state::Actor};
use crate::{
    Error, Result,
    effects::equalizer::{
        EqualizerPreferences, EqualizerPreset, EqualizerSettings, EqualizerStatus,
    },
    playback::{engine::Cancellation, source::ResolvedSource},
};

impl Actor {
    pub(super) fn equalizer_read(&mut self) -> Result<()> {
        match self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::read(&self.store, owner)
        }) {
            Ok(preferences) => {
                if !self.equalizer_initialized
                    || self.equalizer.damaged
                    || preferences.dsp_revision() != self.equalizer.preferences.dsp_revision()
                    || !preferences
                        .settings
                        .same_parameters(self.equalizer.preferences.settings)
                {
                    self.equalizer.status = EqualizerStatus::Pending;
                }
                self.equalizer.preferences = preferences;
                self.equalizer_initialized = true;
                self.equalizer.damaged = false;
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
        Ok(())
    }
    pub(super) async fn equalizer_sync(&mut self) -> Result<()> {
        self.equalizer_read()?;
        // A decoder can hold the engine mutex. An observed pending edit is
        // applied before first unpause in loaded(); source policy is separate
        // and waits for accepted native Play. Do not block load cancellation.
        if self.equalizer.status == EqualizerStatus::Pending && self.pending.is_none() {
            self.equalizer_apply().await?;
        }
        Ok(())
    }
    pub(super) async fn equalizer_change(
        &mut self,
        expected: u64,
        settings: EqualizerSettings,
        preset: Option<EqualizerPreset>,
    ) -> Result<()> {
        let preferences = self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::replace(&self.store, owner, expected, settings, preset)
        })?;
        let changed = !self.equalizer_initialized
            || self.equalizer.damaged
            || (self.equalizer.source_bypassed && settings.requests_activation())
            || preferences.dsp_revision() != self.equalizer.preferences.dsp_revision()
            || !preferences
                .settings
                .same_parameters(self.equalizer.preferences.settings);
        self.equalizer.preferences = preferences;
        self.equalizer_initialized = true;
        self.equalizer.damaged = false;
        if !changed {
            return Ok(());
        }
        self.equalizer.status = EqualizerStatus::Pending;
        if self.pending.is_none() {
            self.equalizer_apply().await?;
        }
        Ok(())
    }
    pub(super) fn equalizer_save_custom(&mut self, expected: u64) -> Result<()> {
        let preferences = self.session.with_local_owner(&self.token, |owner| {
            EqualizerPreferences::save_custom(&self.store, owner, expected)
        })?;
        // This command owns only the saved slot. If another controller changed
        // the active settings, normal preference sync owns applying that change.
        self.equalizer.preferences.saved_custom = preferences.saved_custom;
        if preferences.dsp_revision() == self.equalizer.preferences.dsp_revision()
            && preferences
                .settings
                .same_parameters(self.equalizer.preferences.settings)
        {
            self.equalizer.preferences = preferences;
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
        self.equalizer_apply_configuration(false, self.guard())
            .await
    }

    /// M/308 checks the accepted source after native Play, using the actual
    /// online representation. Ge is a latch, not a permanent format ban.
    pub(super) fn equalizer_source_transition(&self, source: &ResolvedSource) -> Option<bool> {
        if self.equalizer.damaged
            || self.equalizer.status == EqualizerStatus::Failed
            || !self.equalizer.preferences.settings.requests_activation()
        {
            return None;
        }
        let special =
            source.info["kind"] == "online" && super::parametric::special_effect_quality(source);
        // The restore branch also applies to a second special source. Direct
        // equalizer writes can reapply while this latch remains set, just as
        // M/342 re-reads isOn after the full local material write completes.
        // A source superseded during native application can leave a dry
        // adapter without owning the latch. Reconcile at the next valid load.
        (self.equalizer_source_latch || special || self.equalizer.source_bypassed)
            .then_some(special && !self.equalizer_source_latch)
    }

    pub(super) async fn equalizer_apply_configuration(
        &mut self,
        source_bypassed: bool,
        cancel: Cancellation,
    ) -> Result<()> {
        let settings = self.equalizer.preferences.settings.effective();
        let result = super::engine(self.engine.clone(), cancel, move |engine| {
            let apply = if source_bypassed {
                engine.bypass_equalizer_for_source()
            } else {
                engine.apply_equalizer(settings)
            };
            match apply {
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
        self.equalizer.source_bypassed = !self.equalizer.damaged
            && source_bypassed
            && matches!(&result, Ok(EqualizerStatus::Configured));
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
                    self.record_failure(&error);
                }
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) async fn equalizer_clear(&mut self) -> Result<()> {
        self.equalizer = Default::default();
        self.equalizer_initialized = false;
        self.equalizer_source_latch = false;
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
        let source_bypassed = self.equalizer.source_bypassed;
        let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
            match engine.observe_equalizer() {
                Ok(Some(stats)) if stats.failed_blocks == 0 => Ok(
                    if !source_bypassed
                        && stats.active_instances != 0
                        && stats.processed_samples != 0
                        && stats.observed_revision == stats.revision
                    {
                        EqualizerStatus::Processing
                    } else {
                        EqualizerStatus::Configured
                    },
                ),
                Ok(None) => Ok(EqualizerStatus::Configured),
                _ => {
                    // A failed optional filter must not mute the rest of the
                    // song. Keep playback only after its removal succeeds.
                    engine.clear_equalizer()?;
                    Ok(EqualizerStatus::Failed)
                }
            }
        })
        .await;
        self.equalizer.status = result.as_ref().copied().unwrap_or(EqualizerStatus::Failed);
        if self.equalizer.status == EqualizerStatus::Failed {
            self.equalizer.source_bypassed = false;
        }
        result.map(|_| ())
    }
}
