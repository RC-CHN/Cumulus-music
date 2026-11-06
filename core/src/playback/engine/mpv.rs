use super::{
    AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest, LoadedMedia,
    MediaInput, OutputDevices, PinnedReader,
    ffi::{Native, Received},
};
use crate::{
    Error, Result,
    media::ncm::{HeaderPolicy, NcmReader},
};
use std::{
    collections::VecDeque,
    ffi::{CStr, CString},
    path::PathBuf,
    time::{Duration, Instant},
};
mod cold_fallback;
#[cfg(test)]
mod effect_order_tests;
#[cfg(test)]
mod limiter_host_probe;
mod local_equalizer;
mod loudness;
mod parametric;
mod pinned_stop;
#[cfg(test)]
mod stereo_tests;

fn effect_filter(label: &str, graph: String) -> String {
    format!("@cumulus-{label}:lavfi=graph=%{}%{graph}", graph.len())
}

#[derive(Default)]
pub struct MpvConfig {
    /// Explicit native adapter path, never supplied by untrusted page data.
    pub library: Option<PathBuf>,
    /// Decode using the real engine but discard output during verification.
    /// The desktop defaults to the operating system's audio output.
    pub null_output: bool,
}

pub struct MpvEngine {
    native: Option<Native>,
    // Native is destroyed first, so its filter callbacks finish before the
    // owning adapter handle/library are released.
    equalizer: Option<crate::effects::equalizer::EqualizerHost>,
    equalizer_reserved: bool,
    loudness: Option<crate::effects::loudness::LoudnessHost>,
    loudness_reserved: bool,
    parametric: Option<crate::effects::parametric::ParametricHost>,
    equalizer_library: Option<PathBuf>,
    sequence: u64,
    events: VecDeque<EngineEvent>,
    entry: Option<i64>,
    null_output: bool,
    api_version: u64,
    audio_module: Option<super::runtime::MpvModule>,
    retired_local_ready: bool,
    pinned_authority: Option<Cancellation>,
    pinned_bmff_options: bool,
    stereo_pcm: Option<super::StereoPcmControl>,
    local_flac: Option<super::TrustedFlacRuntime>,
    local_wave: bool,
    local_equalizer: Option<local_equalizer::LocalEqualizer>,
}
impl Drop for MpvEngine {
    fn drop(&mut self) {
        let stereo = self.stereo_pcm.take();
        if let Some(control) = &stereo {
            control.cancel();
        }
        if let Some(gate) = self.pinned_authority.take() {
            gate.cancel();
        }
        // Synchronous libmpv destruction acknowledges all callbacks and AO
        // before the held source/graph may be reclaimed for a replacement.
        drop(self.native.take());
        if let Some(control) = stereo {
            control.retire_sink();
        }
    }
}
enum SourceDemuxer<'a> {
    Known(&'a CStr),
    StereoPcm(u32),
}
fn string(value: impl AsRef<[u8]>) -> Result<CString> {
    CString::new(value.as_ref()).map_err(|_| Error::InvalidInput("NUL in audio argument"))
}

impl MpvEngine {
    /// Trusted native launch configuration. Missing/bad components are reported
    /// when enabled; they do not prevent ordinary playback with EQ disabled.
    pub fn with_equalizer_library(mut self, path: PathBuf) -> Self {
        self.equalizer_library = Some(path);
        self
    }
    /// Shared trusted filter library, with independently owned effect handles.
    pub fn with_effects_library(self, path: PathBuf) -> Self {
        self.with_equalizer_library(path)
    }
    /// Attach the trusted native adapter to this player's actual decoder/filter
    /// chain. Opening before a track is allowed; stats distinguish configuration
    /// from actual PCM processing. No network or entitlement side effects.
    pub fn attach_equalizer(
        &mut self,
        library: &std::path::Path,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        if self.equalizer_reserved && self.equalizer_library.as_deref() == Some(library) {
            return self.set_equalizer(settings);
        }
        if self.equalizer.is_some() {
            return Err(Error::State("equalizer is already attached"));
        }
        if self.parametric.is_some() {
            // A missing/failed reservation cannot be reinserted during PEQ:
            // changing the running graph can recreate PEQ instances.
            return Err(Error::State("equalizer must be initialized before PEQ"));
        }
        let host = crate::effects::equalizer::EqualizerHost::open(library, settings)?;
        let filter = effect_filter("geq", host.filter_graph());
        let (filter, loudness) = self.with_loudness_reservation(library, filter);
        self.control(&["af", "add", &filter])?;
        self.adopt_loudness_reservation(loudness);
        self.equalizer = Some(host);
        self.equalizer_library = Some(library.to_path_buf());
        Ok(())
    }
    pub fn set_equalizer(
        &mut self,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        self.equalizer
            .as_mut()
            .ok_or(Error::State("equalizer adapter is not attached"))?
            .update(settings)?;
        if settings.effective().enabled() {
            self.equalizer_reserved = false;
        }
        Ok(())
    }
    pub fn equalizer_stats(&self) -> Result<crate::effects::equalizer::abi::EqualizerStats> {
        self.equalizer
            .as_ref()
            .ok_or(Error::State("equalizer adapter is not attached"))?
            .stats()
    }
    pub fn detach_equalizer(&mut self) -> Result<()> {
        if self.equalizer.is_some() {
            let dry_loudness = self.parametric.is_none() && self.loudness_reserved;
            let labels = if dry_loudness {
                "@cumulus-geq,@cumulus-loudness"
            } else {
                "@cumulus-geq"
            };
            self.control(&["af", "remove", labels])?;
            if dry_loudness {
                self.clear_loudness_reservation();
            }
            self.equalizer = None;
            self.equalizer_reserved = false;
        }
        Ok(())
    }
    pub fn open(config: MpvConfig) -> Result<Self> {
        Self::open_mode(config, false)
    }
    pub fn open_video(
        config: MpvConfig,
        width: u32,
        height: u32,
    ) -> Result<(Self, super::VideoFrames)> {
        super::frames::size(width, height)?;
        let mut engine = Self::open_mode(config, true)?;
        let frames = engine.native_mut()?.start_video(width, height)?;
        Ok((engine, frames))
    }
    fn open_mode(config: MpvConfig, video: bool) -> Result<Self> {
        let mut native = Native::open(config.library.as_deref())?;
        Self::initialize_native(&mut native, config.null_output, video)?;
        Ok(Self {
            api_version: native.version(),
            audio_module: (!video).then(|| native.module()),
            retired_local_ready: false,
            native: Some(native),
            equalizer: None,
            equalizer_reserved: false,
            loudness: None,
            loudness_reserved: false,
            parametric: None,
            equalizer_library: None,
            sequence: 0,
            events: VecDeque::new(),
            entry: None,
            null_output: config.null_output,
            pinned_authority: None,
            pinned_bmff_options: false,
            stereo_pcm: None,
            local_flac: None,
            local_wave: false,
            local_equalizer: None,
        })
    }
    fn initialize_native(native: &mut Native, null_output: bool, video: bool) -> Result<()> {
        for (key, value) in [
            ("config", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            ("terminal", "no"),
            ("msg-level", "all=no"),
            ("idle", "yes"),
            ("keep-open", "no"),
            ("pause", "yes"),
            ("audio-display", "no"),
            ("vo", if video { "libmpv" } else { "null" }),
            ("network-timeout", "15"),
            ("cookies", "no"),
            ("user-agent", "Cumulus/0.1"),
            ("load-unsafe-playlists", "no"),
        ] {
            native.option(&string(key)?, &string(value)?)?;
        }
        // An unset ao selects the operating-system default. "auto" is not a
        // driver in the tested libmpv build (audio initialization error -14).
        if null_output {
            native.option(c"ao", c"null")?;
        }
        native.initialize()?;
        Ok(())
    }
    pub fn api_version(&self) -> u64 {
        self.api_version
    }
    #[cfg(all(test, windows))]
    pub(super) fn proof_runtime_version(&mut self) -> Option<String> {
        self.property(c"mpv-version")
    }
    fn native_mut(&mut self) -> Result<&mut Native> {
        self.native.as_mut().ok_or(Error::Playback {
            operation: "native sink retired",
            code: -1,
        })
    }
    fn sequence(&mut self) -> Result<u64> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::State("audio sequence exhausted"))?;
        Ok(self.sequence)
    }
    fn retain(&mut self, event: EngineEvent) -> Result<()> {
        if self.events.len() >= 2048 {
            return Err(Error::Playback {
                operation: "event backlog",
                code: -1,
            });
        }
        self.events.push_back(event);
        Ok(())
    }
    fn command(
        &mut self,
        args: Vec<CString>,
        deadline: Instant,
        cancel: &Cancellation,
    ) -> Result<()> {
        cancel.check()?;
        let sequence = self.sequence()?;
        self.native_mut()?.command(sequence, &args)?;
        self.await_command(sequence, deadline, cancel)
    }
    fn await_command(
        &mut self,
        sequence: u64,
        deadline: Instant,
        cancel: &Cancellation,
    ) -> Result<()> {
        while Instant::now() < deadline {
            cancel.check()?;
            match self.native_mut()?.event(0.05)? {
                Received::Reply {
                    sequence: returned,
                    error,
                } if returned == sequence => {
                    return if error < 0 {
                        Err(Error::Playback {
                            operation: "command",
                            code: error,
                        })
                    } else {
                        Ok(())
                    };
                }
                Received::Event(event) => self.retain(event)?,
                _ => (),
            }
        }
        Err(Error::Playback {
            operation: "command timeout",
            code: -1,
        })
    }
    fn control(&mut self, args: &[&str]) -> Result<()> {
        self.control_with_cancel(args, &Cancellation::default())
    }
    fn control_with_cancel(&mut self, args: &[&str], cancel: &Cancellation) -> Result<()> {
        self.command(
            args.iter().map(string).collect::<Result<Vec<_>>>()?,
            Instant::now() + Duration::from_secs(5),
            cancel,
        )
    }
    fn control_authorized(&mut self, args: &[&str]) -> Result<()> {
        self.require_pinned_authority()?;
        let gate = self.pinned_authority.clone().unwrap_or_default();
        let result = self.control_with_cancel(args, &gate);
        self.require_pinned_authority()?;
        result
    }
    fn devices(&mut self) -> Result<Vec<super::OutputDevice>> {
        let sequence = self.sequence()?;
        self.native_mut()?.request_devices(sequence)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.native_mut()?.event(0.05)? {
                Received::Devices {
                    sequence: returned,
                    result,
                } if returned == sequence => {
                    return result;
                }
                Received::Event(event) => self.retain(event)?,
                // Late replies to a timed-out query cannot satisfy this one.
                _ => (),
            }
        }
        Err(Error::Playback {
            operation: "audio device query timeout",
            code: -1,
        })
    }
    fn property(&mut self, name: &CStr) -> Option<String> {
        self.native.as_mut()?.property(name)
    }
    fn number(&mut self, name: &CStr) -> Option<f64> {
        self.property(name)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
    fn drain(&mut self) -> Result<Vec<EngineEvent>> {
        for _ in 0..2048 {
            match self.native_mut()?.event(0.0)? {
                Received::None => return Ok(self.events.drain(..).collect()),
                Received::Event(event) => self.retain(event)?,
                _ => (),
            }
        }
        Err(Error::Playback {
            operation: "event backlog",
            code: -1,
        })
    }
    fn source(source: MediaInput) -> Result<CString> {
        match source {
            MediaInput::Url(url) => string(url.as_str()),
            MediaInput::NativeFile { .. } | MediaInput::SearchPublication(_) => {
                Err(Error::InvalidInput("native local file must be pinned"))
            }
            MediaInput::File(path) => {
                let path = std::fs::canonicalize(path)?;
                if !path.is_file() {
                    return Err(Error::InvalidInput("audio source is not a regular file"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    string(path.as_os_str().as_bytes())
                }
                #[cfg(not(unix))]
                {
                    string(
                        path.to_str()
                            .ok_or(Error::InvalidInput("non-Unicode audio path"))?,
                    )
                }
            }
        }
    }
    /// Play an already-opened local audio handle using a one-shot native
    /// stream. No source path or renderer-provided URL is accepted. The
    /// authority predicate runs on decoder threads and must be fast, with
    /// no re-entry into the playback host or engine.
    pub fn load_pinned_audio(
        &mut self,
        mut file: std::fs::File,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        authority.check()?;
        cancel.check()?;
        Self::check_regular(&file)?;
        let format = super::pinned_audio_format(&mut file)?;
        self.load_pinned(Box::new(file), format, authority, cancel)
    }

    #[cfg(test)]
    pub(super) fn load_probe_wav(
        &mut self,
        reader: impl PinnedReader + 'static,
        authority: Cancellation,
    ) -> Result<LoadedMedia> {
        self.control(&["set", "options/cache", "yes"])?;
        self.load_pinned(Box::new(reader), "wav", authority, &Cancellation::default())
    }

    fn check_regular(file: &std::fs::File) -> Result<()> {
        if !file.metadata()?.is_file() {
            return Err(Error::InvalidInput("pinned audio requires a regular file"));
        }
        Ok(())
    }

    fn load_pinned(
        &mut self,
        reader: Box<dyn PinnedReader>,
        format: &'static str,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        authority.check()?;
        cancel.check()?;
        self.stop()?;
        self.restore_bmff_options()?;
        // The sink owns a per-load gate, never the caller's account/focus
        // lifetime. Retiring this source cannot cancel another authorized one.
        let lifetime = Cancellation::new(move || !authority.is_cancelled());
        let result = (|| {
            if format == "mov" {
                // libmpv replaces AVFormatContext.io_open with a deny-all
                // callback; FFmpeg must not accept aliases either.
                self.pinned_bmff_options = true;
                self.control(&["set", "options/access-references", "no"])?;
                self.control(&["set", "options/autoload-files", "no"])?;
                self.control(&[
                    "set",
                    "options/demuxer-lavf-o",
                    "enable_drefs=0,use_absolute_path=0,protocol_whitelist=cumuluspinned",
                ])?;
            }
            let source = self.native_mut()?.stage_pinned(reader, lifetime.clone())?;
            self.pinned_authority = Some(lifetime.clone());
            let load_lifetime = lifetime.clone();
            let load_cancel = cancel.clone();
            let guard = Cancellation::new(move || {
                !load_lifetime.is_cancelled() && !load_cancel.is_cancelled()
            });
            // Use file-local demuxer settings so ordinary playback gets the
            // real defaults back. mpv 0.37 rejects an empty lavf format.
            let media = self.load_source(
                source,
                Some(SourceDemuxer::Known(&string(format)?)),
                0.0,
                None,
                Duration::from_secs(25),
                &guard,
            )?;
            if format == "mov" && media.codec.as_deref() != Some("aac") {
                return Err(Error::InvalidInput("unsupported pinned audio format"));
            }
            lifetime.check()?;
            Ok(media)
        })();
        if let Some(native) = &self.native {
            native.clear_staged_pinned();
        }
        if result.is_err() {
            let cancelled = lifetime.is_cancelled() || cancel.is_cancelled();
            let _ = self.stop();
            if cancelled {
                return Err(Error::Cancelled);
            }
        }
        result
    }

    /// Explicit native-only source handoff. The actual held-source producer
    /// established stereo/format/authority; this does not activate a material
    /// setting. Other effect nodes must be absent until full ordering is proven.
    pub fn load_stereo_pcm(
        &mut self,
        reader: super::StereoPcmReader,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if self.entry.is_some()
            || self.pinned_authority.is_some()
            || self.equalizer.is_some()
            || self.loudness.is_some()
            || self.parametric.is_some()
        {
            return Err(Error::State(
                "stereo PCM requires an idle plain native sink",
            ));
        }
        let control = reader.control();
        control.validate_host()?;
        cancel.check()?;
        self.restore_bmff_options()?;
        let rate = reader.rate();
        let authority = control.authority();
        control.publish_sink()?;
        let source = match self
            .native_mut()
            .and_then(|native| native.stage_stereo_pcm(reader, authority.clone()))
        {
            Ok(source) => source,
            Err(error) => {
                control.retire_sink();
                return Err(error);
            }
        };
        self.pinned_authority = Some(authority.clone());
        let load_authority = authority.clone();
        let load_cancel = cancel.clone();
        let guard = Cancellation::new(move || {
            !load_authority.is_cancelled() && !load_cancel.is_cancelled()
        });
        let result = self
            .load_source(
                source,
                Some(SourceDemuxer::StereoPcm(rate)),
                0.0,
                None,
                Duration::from_secs(25),
                &guard,
            )
            .and_then(|media| {
                control.validate_host()?;
                cancel.check()?;
                if media.codec.as_deref() != Some("pcm_f32le") {
                    return Err(Error::Media("raw stereo PCM codec was not confirmed"));
                }
                control.bind_entry(media.entry_id)?;
                Ok(media)
            });
        if let Some(native) = &self.native {
            native.clear_staged_pinned();
        }
        if result.is_err() {
            control.cancel();
            self.retire_pinned();
            control.retire_sink();
        } else {
            self.stereo_pcm = Some(control);
        }
        result
    }

    fn restore_bmff_options(&mut self) -> Result<()> {
        if self.pinned_bmff_options {
            self.control(&["set", "options/access-references", "yes"])?;
            self.control(&["set", "options/autoload-files", "yes"])?;
            self.control(&["set", "options/demuxer-lavf-o", ""])?;
            self.pinned_bmff_options = false;
        }
        Ok(())
    }

    fn require_pinned_authority(&mut self) -> Result<()> {
        if self
            .pinned_authority
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
        {
            self.retire_pinned();
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    fn load_inner(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        request.validate()?;
        cancel.check()?;
        let source = match request.source {
            MediaInput::NativeFile { path, fingerprint } => {
                if request.end.is_some() {
                    return Err(Error::InvalidInput(
                        "native local file cannot have a segment",
                    ));
                }
                let (file, format) =
                    crate::library::native_open::checked_audio_file(&path, &fingerprint)?;
                return self.load_open_file(
                    file,
                    format,
                    Cancellation::default(),
                    request.start,
                    cancel,
                );
            }
            source => Self::source(source)?,
        };
        if self.pinned_bmff_options || self.pinned_authority.is_some() {
            self.stop()?;
            self.restore_bmff_options()?;
        }
        self.load_source(
            source,
            None,
            request.start,
            request.end,
            request.timeout,
            cancel,
        )
    }

    fn load_source(
        &mut self,
        source: CString,
        forced_demuxer: Option<SourceDemuxer<'_>>,
        start: f64,
        end: Option<f64>,
        timeout: Duration,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        cancel.check()?;
        if self.stereo_pcm.is_some() {
            self.stop()?;
        }
        let deadline = Instant::now() + timeout;
        self.command(vec![string("stop")?], deadline, cancel)?;
        self.entry = None;
        self.drain()?;
        for (key, value) in [
            ("pause", "yes".into()),
            ("options/start", start.to_string()),
            (
                "options/end",
                end.map(|n| n.to_string()).unwrap_or("none".into()),
            ),
        ] {
            self.command(
                vec![string("set")?, string(key)?, string(value)?],
                deadline,
                cancel,
            )?;
        }
        if let Some(format) = forced_demuxer {
            cancel.check()?;
            let sequence = self.sequence()?;
            match format {
                SourceDemuxer::Known(format) => self
                    .native_mut()?
                    .load_with_demuxer(sequence, &source, format)?,
                SourceDemuxer::StereoPcm(rate) => self
                    .native_mut()?
                    .load_with_stereo_pcm(sequence, &source, rate)?,
            }
            self.await_command(sequence, deadline, cancel)?;
        } else {
            self.command(
                vec![string("loadfile")?, source, string("replace")?],
                deadline,
                cancel,
            )?;
        }
        let mut entry = None;
        while Instant::now() < deadline {
            cancel.check()?;
            let event = if let Some(event) = self.events.pop_front() {
                Some(event)
            } else {
                match self.native_mut()?.event(0.05)? {
                    Received::Event(event) => Some(event),
                    _ => None,
                }
            };
            match event {
                Some(EngineEvent::Started { entry_id }) => entry = Some(entry_id),
                Some(EngineEvent::Loaded) if entry.is_some() => {
                    let entry_id = entry.expect("checked above");
                    self.entry = Some(entry_id);
                    return Ok(LoadedMedia {
                        entry_id,
                        duration: self.number(c"duration").filter(|v| *v > 0.0),
                        codec: self.property(c"audio-codec-name"),
                    });
                }
                Some(EngineEvent::Ended {
                    entry_id,
                    reason,
                    error,
                }) if Some(entry_id) == entry => {
                    return Err(Error::Playback {
                        operation: if reason == EndReason::Redirect {
                            "unexpected playlist"
                        } else {
                            "load ended"
                        },
                        code: error,
                    });
                }
                Some(EngineEvent::Shutdown) => {
                    return Err(Error::Playback {
                        operation: "shutdown",
                        code: -1,
                    });
                }
                _ => (),
            }
        }
        Err(Error::Playback {
            operation: "load timeout",
            code: -1,
        })
    }
}

impl AudioEngine for MpvEngine {
    fn decoded_audio_format(&mut self, entry_id: i64) -> Result<Option<super::DecodedAudioFormat>> {
        self.require_pinned_authority()?;
        if self.entry != Some(entry_id) {
            return Err(Error::StaleResponse);
        }
        if self.property(c"idle-active").as_deref() != Some("no") {
            return Ok(None);
        }
        if self.property(c"seeking").as_deref() != Some("no") {
            return Ok(None);
        }
        let value = self.native_mut()?.decoded_audio_format()?;
        self.require_pinned_authority()?;
        if self.entry != Some(entry_id) {
            return Err(Error::StaleResponse);
        }
        if self.property(c"seeking").as_deref() != Some("no") {
            return Ok(None);
        }
        Ok(
            value.map(|(sample_rate, channels)| super::DecodedAudioFormat {
                entry_id,
                sample_rate,
                channels,
            }),
        )
    }
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        if self.null_output {
            return Ok(None);
        }
        let devices = self.devices()?;
        let configured = self
            .property(c"audio-device")
            .ok_or(Error::Media("audio device preference unavailable"))?;
        Ok(Some(OutputDevices {
            devices,
            configured,
            active_driver: self.property(c"current-ao"),
        }))
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        if self.null_output {
            return Err(Error::State("audio output is isolated"));
        }
        if !super::output::valid_id(id) {
            return Err(Error::InvalidInput("invalid audio device identifier"));
        }
        // Do not pass arbitrary device/driver strings to mpv. Refresh on every
        // change so a UI's stale device list cannot select a removed endpoint.
        if !self.devices()?.iter().any(|device| device.id == id) {
            return Err(Error::State("audio device is no longer available"));
        }
        // An acknowledged property write only schedules reinitialization. It
        // neither unpauses nor reloads a file, and is not activation evidence.
        self.control(&["set", "audio-device", id])
    }
    fn apply_parametric_component(
        &mut self,
        plan: crate::effects::parametric::ParametricPlan,
    ) -> Result<bool> {
        if self.local_equalizer.is_some() {
            return Ok(false);
        }
        if !plan.enabled() {
            self.bypass_parametric_component()?;
            return Ok(true);
        }
        self.detach_parametric_equalizer()?;
        let Some(path) = self.equalizer_library.clone() else {
            return Ok(false);
        };
        self.attach_parametric_equalizer(&path, plan)?;
        Ok(true)
    }
    fn bypass_parametric_component(&mut self) -> Result<()> {
        if let Some(host) = &self.parametric {
            host.bypass()?;
        }
        Ok(())
    }
    fn restore_parametric_component(
        &mut self,
        plan: crate::effects::parametric::ParametricPlan,
    ) -> Result<()> {
        self.parametric
            .as_ref()
            .ok_or(Error::State("parametric equalizer adapter is not attached"))?
            .restore(plan)
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        self.detach_parametric_equalizer()
    }
    fn apply_equalizer(
        &mut self,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<bool> {
        let settings = settings.effective();
        let Some(path) = self.equalizer_library.clone() else {
            return Ok(false);
        };
        if !settings.enabled()
            && !self.equalizer_reserved
            && self
                .equalizer
                .as_ref()
                .is_some_and(|h| h.stats().is_ok_and(|s| s.active_instances == 0))
        {
            // A filter rejected during decoder initialization must be removable
            // by switching EQ off, so ordinary playback can recover.
            self.detach_equalizer()?;
        }
        if self
            .equalizer
            .as_ref()
            .is_some_and(|h| h.stats().map_or(true, |s| s.failed_blocks != 0))
        {
            self.detach_equalizer()?;
        }
        if self.equalizer.is_some() {
            if !self.equalizer_reserved || settings.enabled() {
                self.set_equalizer(settings)?;
            }
        } else if settings.enabled() {
            self.attach_equalizer(&path, settings)?;
        }
        Ok(!settings.tone_active() || self.equalizer.as_ref().is_some_and(|h| h.supports_tone()))
    }
    fn bypass_equalizer_for_source(&mut self) -> Result<bool> {
        if self.equalizer_library.is_none() {
            return Ok(false);
        }
        if let Some(host) = &self.equalizer {
            if host.stats()?.failed_blocks != 0 {
                return Err(Error::EqualizerProcessorFailed);
            }
            // Native Play can acknowledge before the new decoder has an
            // active LADSPA instance. Preserve the handle and effect order;
            // the ordinary manual-disable recovery may remove it at zero.
            self.set_equalizer(Default::default())?;
        }
        Ok(true)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        self.detach_equalizer()
    }
    fn observe_equalizer(
        &mut self,
    ) -> Result<Option<crate::effects::equalizer::abi::EqualizerStats>> {
        if self.equalizer_reserved {
            return Ok(None);
        }
        self.equalizer.as_ref().map(|h| h.stats()).transpose()
    }
    fn apply_loudness(
        &mut self,
        settings: crate::effects::loudness::LoudnessSettings,
    ) -> Result<bool> {
        if self.local_equalizer.is_some() {
            return Ok(false);
        }
        self.configure_loudness(settings)
    }
    fn clear_loudness(&mut self) -> Result<()> {
        self.detach_loudness()
    }
    fn reset_track_loudness(&mut self) -> Result<()> {
        self.reset_loudness_for_load()
    }
    fn observe_loudness(&mut self) -> Result<Option<crate::effects::loudness::abi::LoudnessStats>> {
        if self.loudness_reserved {
            return Ok(None);
        }
        self.loudness.as_ref().map(|h| h.stats()).transpose()
    }
    fn reopen_retired_local_sink(&mut self, cancel: &Cancellation) -> Result<bool> {
        if !self.retired_local_ready {
            return Ok(false);
        }
        if self.native.is_some()
            || self.entry.is_some()
            || self.pinned_authority.is_some()
            || self.stereo_pcm.is_some()
            || self.local_equalizer.is_some()
        {
            return Err(Error::State("retired native sink still owns playback"));
        }
        let module = self
            .audio_module
            .as_ref()
            .ok_or(Error::State("retired native audio SDK is unavailable"))?;
        cancel.check()?;
        // Keep the already mapped SDK. A path lookup at recovery time could
        // select a different runtime; no old Native/source survives this call.
        let mut native = Native::from_runtime(module.acquire()?)?;
        Self::initialize_native(&mut native, self.null_output, false)?;
        cancel.check()?;
        self.native = Some(native);
        self.retired_local_ready = false;
        Ok(true)
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let result = self.load_inner(request, cancel);
        if result.is_err() {
            let _ = self.stop();
        }
        result
    }
    fn load_with_local_equalizer(
        &mut self,
        request: LoadRequest,
        settings: crate::effects::equalizer::EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let result = self.load_local_equalizer(request, settings, cancel);
        if result.is_err() {
            let _ = self.stop();
        }
        result
    }
    fn supports_local_equalizer(&self) -> bool {
        self.local_flac.is_some() || self.local_wave
    }
    fn supports_indexed_flac_equalizer(&self) -> bool {
        cfg!(all(target_os = "linux", target_arch = "x86_64")) && self.local_flac.is_some()
    }
    fn load_indexed_flac_equalizer(
        &mut self,
        receipt: crate::library::IndexedFlacReceipt,
        selected: crate::library::LocalResource,
        settings: crate::effects::equalizer::EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let result = self.load_indexed_local_equalizer(receipt, selected, settings, cancel);
        if result.is_err() {
            let _ = self.stop();
        }
        result
    }
    fn local_equalizer_entry(&self) -> Option<i64> {
        self.local_equalizer.as_ref().and(self.entry)
    }
    fn pump_local_equalizer(&mut self) -> Result<()> {
        self.observe_local_equalizer_clock()
    }
    fn apply_local_equalizer(
        &mut self,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        self.update_local_equalizer(settings)
    }
    fn local_equalizer_processing(&mut self) -> Result<bool> {
        self.observe_local_equalizer_processing()
    }
    fn yield_local_equalizer(&mut self, entry: i64, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.resume_stock_for_independent_filter(entry, cancel)
    }
    fn streaming_audio_available(&mut self) -> bool {
        self.native_mut()
            .and_then(Native::prepare_pinned_stream)
            .is_ok()
    }

    fn load_streaming_audio(
        &mut self,
        reader: crate::playback::StreamingAudioReader,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let authority = reader.authority();
        let acceptance = reader.decoder_acceptance();
        let media = self.load_pinned(Box::new(reader), "mp3", authority.clone(), cancel)?;
        if media.codec.as_deref() != Some("mp3") {
            self.stop()?;
            return Err(Error::Response("native streaming source is not MP3"));
        }
        authority.check()?;
        if let Some(acceptance) = acceptance {
            acceptance.confirm();
        }
        Ok(media)
    }

    fn supports_pinned_search_download(&self) -> bool {
        self.native.is_some()
    }

    fn load_pinned_audio(
        &mut self,
        file: std::fs::File,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        MpvEngine::load_pinned_audio(self, file, authority, cancel)
    }

    fn load_pinned_ncm(
        &mut self,
        file: std::fs::File,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        authority.check()?;
        cancel.check()?;
        Self::check_regular(&file)?;
        let mut reader = NcmReader::new(file, HeaderPolicy::Strict)?;
        let format = super::pinned_ncm_format(&mut reader)?;
        self.load_pinned(Box::new(reader), format, authority, cancel)
    }

    fn supports_cold_limiter(&self) -> bool {
        self.native.is_some()
    }

    fn load_cold_limiter(
        &mut self,
        wave: super::ColdLimiterWave,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if self.entry.is_some() || self.pinned_authority.is_some() {
            return Err(Error::State("cold limiter requires an idle sink"));
        }
        wave.authority.validate()?;
        let source = wave.authority.cancellation();
        let media = self.load_pinned(
            Box::new(std::io::Cursor::new(wave.data)),
            "wav",
            source,
            cancel,
        )?;
        let result = wave
            .authority
            .validate()
            .and_then(|()| cancel.check())
            .and_then(|()| {
                if media.codec.as_deref() != Some("pcm_f32le") {
                    return Err(Error::Media(
                        "cold limiter native PCM format was not confirmed",
                    ));
                }
                self.confirm_cold_position(
                    media.entry_id,
                    0.0,
                    Instant::now() + Duration::from_secs(3),
                    cancel,
                )?;
                Ok(media)
            });
        if result.is_err() {
            self.retire_pinned();
        }
        result
    }

    fn load_cold_stock_flac(
        &mut self,
        reader: crate::download::offline::ColdOfflineFlacReader,
        position: f64,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if position == 0.0 {
            if self.entry.is_some() || self.pinned_authority.is_some() {
                return Err(Error::State("cold FLAC fallback requires an idle sink"));
            }
            let authority = reader.authority();
            authority.validate()?;
            let media =
                self.load_pinned(Box::new(reader), "flac", authority.cancellation(), cancel)?;
            if let Err(error) = authority.validate().and_then(|()| cancel.check()) {
                self.retire_pinned();
                return Err(error);
            }
            return Ok(media);
        }
        self.resume_cold_flac_native(reader, position, true, cancel)
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        self.control_authorized(&["set", "pause", if paused { "yes" } else { "no" }])
    }
    fn seek(&mut self, seconds: f64) -> Result<()> {
        if self.stereo_pcm.is_some() {
            return Err(Error::State(
                "stereo PCM seek requires a retired-source handoff",
            ));
        }
        if !seconds.is_finite() || seconds < 0.0 {
            self.require_pinned_authority()?;
            return Err(Error::InvalidInput("invalid audio seek"));
        }
        self.control_authorized(&["seek", &seconds.to_string(), "absolute+exact"])
    }
    fn seek_with_receipt(
        &mut self,
        seconds: f64,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<Option<LoadedMedia>> {
        if self.local_equalizer.is_none() {
            self.seek(seconds)?;
            return Ok(None);
        }
        self.seek_local_equalizer(seconds, entry, cancel).map(Some)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(Error::InvalidInput("volume outside 0..100"));
        }
        self.control(&["set", "volume", &value.to_string()])
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        if !value.is_finite() || !(0.25..=4.0).contains(&value) {
            return Err(Error::InvalidInput("speed outside 0.25..4"));
        }
        if let (Some(entry), Some(control), Some(_)) = (
            self.entry,
            self.stereo_pcm.clone(),
            self.local_equalizer.as_ref(),
        ) {
            // Admit the bounded reserve before mpv can start demanding the
            // higher rate. If the command fails, reconcile against the exact
            // native property, using the old reserve if it is unavailable.
            let before = control.set_local_playback_speed(entry, value)?;
            let result = self.control_authorized(&["set", "speed", &value.to_string()]);
            if result.is_err() {
                let observed = self
                    .number(c"speed")
                    .filter(|speed| (0.25..=4.0).contains(speed))
                    .unwrap_or(before);
                let _ = control.set_local_playback_speed(entry, observed);
            }
            return result;
        }
        self.control(&["set", "speed", &value.to_string()])
    }
    fn stop(&mut self) -> Result<()> {
        if self.pinned_authority.is_some() {
            self.retire_pinned();
            self.local_equalizer = None;
            return Ok(());
        }
        self.local_equalizer = None;
        let Some(native) = &self.native else {
            return Ok(()); // destruction has already completed
        };
        native.clear_staged_pinned();
        self.control(&["stop"])?;
        self.pinned_authority = None;
        self.entry = None;
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        if let Some(control) = self.stereo_pcm.clone()
            && let Err(error) = control.validate_host()
        {
            self.retire_pinned();
            return Err(error);
        }
        self.require_pinned_authority()?;
        // The callback quarantines a failed block. Report its exact processor
        // so the playback actor can remove that node before accepting a clock.
        if let Some(loudness) = &self.loudness
            && !loudness.stats().is_ok_and(|stats| stats.failed_blocks == 0)
        {
            return Err(Error::LoudnessProcessorFailed);
        }
        if let Some(equalizer) = &self.equalizer
            && !equalizer
                .stats()
                .is_ok_and(|stats| stats.failed_blocks == 0)
        {
            return Err(Error::EqualizerProcessorFailed);
        }
        let parametric_samples = if let Some(parametric) = &self.parametric {
            let stats = parametric
                .stats()
                .ok()
                .filter(|stats| stats.failed_blocks == 0)
                .ok_or(Error::ParametricProcessorFailed)?;
            (stats.active_instances > 0 && stats.observed_revision == stats.revision)
                .then_some(stats.processed_samples)
        } else {
            None
        };
        let events = self.drain()?;
        // These native properties are independent reads. If the seek settles
        // between them, the clock could still have been sampled before it did.
        let seeking_before = self.property(c"seeking").as_deref() != Some("no");
        let position = self.number(c"time-pos");
        let seeking = seeking_before || self.property(c"seeking").as_deref() != Some("no");
        let mut snapshot = EngineSnapshot {
            entry_id: self.entry,
            position,
            duration: self.number(c"duration"),
            // Advisory demuxer timestamp; never use it for seeking or rights.
            buffered_through: (!seeking)
                .then(|| self.number(c"demuxer-cache-time"))
                .flatten(),
            paused: self.property(c"pause").as_deref() == Some("yes"),
            buffering: self.property(c"paused-for-cache").as_deref() == Some("yes"),
            seeking,
            volume: self.number(c"volume"),
            speed: self.number(c"speed"),
            audio_output: self.property(c"current-ao"),
            parametric_samples,
            events,
        };
        // Events and properties may have been collected while the authority
        // changed. Do not let a concurrent revoke publish a stale EOF/clock.
        self.require_pinned_authority()?;
        if let Some(control) = &self.stereo_pcm
            && let Some(entry) = snapshot.entry_id
        {
            control.observe(entry, snapshot.position, snapshot.paused, snapshot.seeking)?;
            if self.local_equalizer.is_some() {
                let pcm = control.snapshot()?;
                let rate = f64::from(control.rate());
                snapshot.position = snapshot
                    .position
                    .map(|_| pcm.native_source_frame as f64 / rate);
                snapshot.duration = Some(pcm.source_end as f64 / rate);
                snapshot.buffered_through = None;
            }
        }
        if self.entry.is_some_and(|active| snapshot.events.iter().any(|event| {
                matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Eof | EndReason::Stop, error: 0 } if *entry_id == active)
            }))
            && let Some(gate) = self.pinned_authority.take()
        {
            // Its terminal event was observed while still authorized. There
            // is no pinned entry left to stop on the next ordinary load.
            gate.cancel();
            if let Some(control) = self.stereo_pcm.take() { control.retire_sink(); control.cancel(); }
            self.local_equalizer = None;
        }
        Ok(snapshot)
    }
}
