//! Combined M3091 material on one held, explicitly opened FLAC or PCM WAV.
use super::{
    AudioEngine, Cancellation, Error, LoadRequest, LoadedMedia, MediaInput, MpvEngine, Result,
};
use crate::{
    effects::{equalizer::EqualizerSettings, stereo::LocalEqualizerMaterialPlan},
    playback::engine::{
        NativeStereoPass, NativeStereoSelection, NativeStereoSource, NativeStereoStart,
        TrustedFlacRuntime,
    },
};
use std::{fs::File, time::Duration};

const CONTROL_WAIT: Duration = Duration::from_secs(2);

pub(super) struct LocalEqualizer {
    pass: Box<NativeStereoPass>,
    original_demuxer: &'static str,
    settings: EqualizerSettings,
    // Only claim the new settings after the native clock has passed audio
    // which could already have been queued when the worker applied them.
    watermark: u64,
    // The worker can finish while the native sink still owns queued PCM.
    // Keep newly saved controls for a later seek without cutting that tail.
    pending: bool,
}

impl MpvEngine {
    pub fn with_local_equalizer_decoder(mut self, runtime: TrustedFlacRuntime) -> Self {
        self.local_flac = Some(runtime);
        self
    }

    /// Opt in to the built-in, strictly validated native PCM WAV source.
    /// No separately installed decoder or path input is required.
    pub fn with_local_equalizer_pcm_wave(mut self) -> Self {
        self.local_wave = true;
        self
    }

    pub(super) fn load_open_file(
        &mut self,
        file: File,
        format: &'static str,
        authority: Cancellation,
        start: f64,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let loaded = self.load_pinned(Box::new(file), format, authority, cancel)?;
        cancel.check()?;
        if start > 0.0 {
            self.seek(start)?;
        }
        Ok(loaded)
    }

    pub(super) fn load_local_equalizer(
        &mut self,
        request: LoadRequest,
        settings: EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        request.validate()?;
        cancel.check()?;
        let MediaInput::NativeFile { path, fingerprint } = &request.source else {
            return self.load_inner(request, cancel);
        };
        if request.start != 0.0
            || request.end.is_some()
            || !settings.local_graph_requested()
            || self.parametric.is_some()
            || (self.loudness.is_some() && !self.loudness_reserved)
        {
            return self.load_inner(request, cancel);
        }
        let (file, format) = crate::library::native_open::checked_audio_file(path, fingerprint)?;
        self.load_held_local_equalizer(file, format, settings, cancel, false)
    }

    pub(super) fn load_indexed_local_equalizer(
        &mut self,
        receipt: crate::library::IndexedFlacReceipt,
        selected: crate::library::LocalResource,
        settings: EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        cancel.check()?;
        let file = receipt.into_ordinary_file(&selected, cancel)?;
        if !settings.local_graph_requested()
            || self.parametric.is_some()
            || (self.loudness.is_some() && !self.loudness_reserved)
        {
            return self.load_open_file(file, "flac", cancel.clone(), 0.0, cancel);
        }
        self.load_held_local_equalizer(file, "flac", settings, cancel, true)
    }

    fn load_held_local_equalizer(
        &mut self,
        file: File,
        format: &'static str,
        settings: EqualizerSettings,
        cancel: &Cancellation,
        indexed: bool,
    ) -> Result<LoadedMedia> {
        let authority = cancel.clone();
        let plan = LocalEqualizerMaterialPlan::from_saved(settings);
        let selection = if format == "wav" && self.local_wave {
            NativeStereoSource::select_wave(file, authority, plan)?
        } else {
            let Some(library) = self
                .local_flac
                .as_ref()
                .and_then(|runtime| runtime.verified_path().ok())
            else {
                // This is the old load_inner stock authority. A verified WAV
                // capability must not make a non-graph MP3/FLAC load depend on
                // the preparation request after it has already committed.
                let authority = if indexed {
                    authority
                } else {
                    Cancellation::default()
                };
                return self.load_open_file(file, format, authority, 0.0, cancel);
            };
            if format != "flac" {
                return self.load_open_file(file, format, authority, 0.0, cancel);
            }
            if indexed {
                NativeStereoSource::select_indexed(file, authority, library, plan)?
            } else {
                NativeStereoSource::select(file, authority, library, plan)?
            }
        };
        let source = match selection {
            NativeStereoSelection::Qualified(source) => source,
            NativeStereoSelection::Stock(held) => {
                let (file, authority) = held.into_stock()?;
                return self.load_open_file(file, format, authority, 0.0, cancel);
            }
        };
        let total = source.total_frames();
        match source.start(0, cancel)? {
            NativeStereoStart::Ready(pass) => {
                // Most callers already retired their previous source before
                // resolution. Preserve the same replacement contract for
                // native clients that call this engine directly.
                self.stop()?;
                // No ordinary filter may process the new graph a second
                // time. Only our ordinary GEQ/dry reservation is displaced.
                self.detach_equalizer()?;
                if self.loudness_reserved {
                    self.detach_loudness()?;
                }
                self.load_local_equalizer_pass(pass, format, settings, total, true, cancel)
            }
            NativeStereoStart::Stock(source) => {
                let (file, authority) = source.into_reader()?.into_stock()?;
                self.load_open_file(file, format, authority, 0.0, cancel)
            }
        }
    }

    fn load_local_equalizer_pass(
        &mut self,
        mut pass: Box<NativeStereoPass>,
        original_demuxer: &'static str,
        settings: EqualizerSettings,
        total: u64,
        allow_unplayed_fallback: bool,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let rate = pass.control().rate();
        let reader = pass.take_reader()?;
        let mut media = match self.load_stereo_pcm(reader, cancel) {
            Ok(media) => media,
            Err(error) if allow_unplayed_fallback && self.native.is_some() => {
                cancel.check()?;
                if let Ok(source) = pass.reclaim() {
                    let (file, authority) = source.into_reader()?.into_stock()?;
                    return self.load_open_file(file, original_demuxer, authority, 0.0, cancel);
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        media.duration = Some(total as f64 / f64::from(rate));
        self.local_equalizer = Some(LocalEqualizer {
            pass,
            original_demuxer,
            settings,
            watermark: 0,
            pending: false,
        });
        Ok(media)
    }

    pub(super) fn seek_local_equalizer(
        &mut self,
        seconds: f64,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if self.entry != Some(entry) {
            return Err(Error::StaleResponse);
        }
        cancel.check()?;
        self.require_pinned_authority()?;
        let local = self.local_equalizer.as_ref().expect("local seek selected");
        let control = local.pass.control();
        let total = control.snapshot()?.source_end;
        let frame = (seconds * f64::from(control.rate())).floor();
        if !frame.is_finite() || frame < 0.0 || frame > total as f64 {
            return Err(Error::InvalidInput("invalid local equalizer seek"));
        }
        // A seek to the displayed end still needs one real source frame for
        // the native decoder to load and emit a genuine EOF.
        let frame = (frame as u64).min(total.saturating_sub(1));
        let playback_speed = self
            .number(c"speed")
            .filter(|rate| (0.25..=4.0).contains(rate));
        let retirement = self.retire_local_equalizer_pinned(entry, cancel);
        let LocalEqualizer {
            pass,
            original_demuxer,
            settings,
            pending,
            ..
        } = self.local_equalizer.take().expect("local seek selected");
        retirement?;
        cancel.check()?;
        let mut source = pass.reclaim()?;
        self.native_mut()?;
        cancel.check()?;
        if pending {
            source.configure_reclaimed(
                LocalEqualizerMaterialPlan::from_saved(settings),
                settings.master_bypassed(),
            )?;
        }
        match source.start(frame, cancel)? {
            NativeStereoStart::Ready(pass) => {
                let media = self.load_local_equalizer_pass(
                    pass,
                    original_demuxer,
                    settings,
                    total,
                    false,
                    cancel,
                )?;
                if let Some(speed) = playback_speed
                    && let Err(error) = self.speed(speed)
                {
                    let _ = self.stop();
                    return Err(error);
                }
                Ok(media)
            }
            NativeStereoStart::Stock(_) => {
                // No replay or silent graph change after an audible entry.
                Err(Error::Media("local equalizer seek could not prepare PCM"))
            }
        }
    }

    pub(super) fn resume_stock_for_independent_filter(
        &mut self,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if self.local_equalizer_entry() != Some(entry) {
            return Err(Error::StaleResponse);
        }
        cancel.check()?;
        self.pause(true)?;
        self.observe_local_equalizer_clock()?;
        let control = self
            .local_equalizer
            .as_ref()
            .expect("checked exact entry")
            .pass
            .control();
        let snapshot = control.snapshot()?;
        let first = snapshot
            .native_source_frame
            .min(snapshot.source_end.saturating_sub(1));
        let retirement = self.retire_local_equalizer_pinned(entry, cancel);
        let local = self.local_equalizer.take().expect("checked exact entry");
        retirement?;
        cancel.check()?;
        let (file, authority) = local.pass.reclaim()?.into_reader()?.into_stock()?;
        self.native_mut()?;
        self.load_open_file(
            file,
            local.original_demuxer,
            authority,
            first as f64 / f64::from(control.rate()),
            cancel,
        )
    }

    pub(super) fn update_local_equalizer(&mut self, settings: EqualizerSettings) -> Result<()> {
        self.require_pinned_authority()?;
        let local = self
            .local_equalizer
            .as_mut()
            .ok_or(Error::State("native local equalizer is unavailable"))?;
        let plan = LocalEqualizerMaterialPlan::from_saved(settings);
        if local.settings.master_bypassed() == settings.master_bypassed()
            && LocalEqualizerMaterialPlan::from_saved(local.settings) == plan
        {
            local.settings = settings;
            return Ok(());
        }
        let control = local.pass.control();
        let apply = || -> Result<()> {
            let revision = if settings.master_bypassed() {
                local.pass.disable_master()?
            } else {
                local.pass.apply_material(plan)?
            };
            control.wait_applied(revision, CONTROL_WAIT)
        };
        let before = control.snapshot()?;
        if before.failed || before.cancelled {
            return Err(Error::Media("native local equalizer processing failed"));
        }
        if before.graph_end {
            local.pending = true;
            local.settings = settings;
            return Ok(());
        }
        if let Err(error) = apply() {
            let after = control.snapshot()?;
            if !after.graph_end
                || after.failed
                || after.cancelled
                || matches!(error.root(), Error::Cancelled | Error::StaleResponse)
            {
                return Err(error);
            }
            local.pending = true;
            local.settings = settings;
            return Ok(());
        }
        local.watermark = control.snapshot()?.produced_frames;
        local.settings = settings;
        Ok(())
    }

    pub(super) fn observe_local_equalizer_clock(&mut self) -> Result<()> {
        if self.local_equalizer.is_none() {
            return Ok(());
        }
        self.require_pinned_authority()?;
        let entry = self
            .entry
            .ok_or(Error::State("local equalizer entry is missing"))?;
        let seeking_before = self.property(c"seeking").as_deref() != Some("no");
        let position = self.number(c"time-pos");
        let seeking = seeking_before || self.property(c"seeking").as_deref() != Some("no");
        let paused = self.property(c"pause").as_deref() == Some("yes");
        self.stereo_pcm
            .as_ref()
            .ok_or(Error::State("local equalizer sink is missing"))?
            .observe(entry, position, paused, seeking)
    }

    pub(super) fn observe_local_equalizer_processing(&mut self) -> Result<bool> {
        let Some(local) = &self.local_equalizer else {
            return Ok(false);
        };
        let snapshot = local.pass.control().snapshot()?;
        if snapshot.failed {
            return Err(Error::Media("native local equalizer processing failed"));
        }
        Ok(local.settings.local_graph_requested()
            && !local.pending
            && !snapshot.cancelled
            && !snapshot.paused
            && !snapshot.seeking
            && snapshot.native_output_frame > local.watermark)
    }
}
