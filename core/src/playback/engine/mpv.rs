use super::{
    AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest, LoadedMedia,
    MediaInput,
    ffi::{Native, Received},
};
use crate::{Error, Result};
use std::{
    collections::VecDeque,
    ffi::{CStr, CString, OsStr},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct MpvConfig {
    /// Explicit native adapter path, never supplied by untrusted page data.
    pub library: Option<PathBuf>,
    /// Decode using the real engine but discard output during verification.
    /// The desktop defaults to the operating system's audio output.
    pub null_output: bool,
}

pub struct MpvEngine {
    native: Native,
    // Native is destroyed first, so its filter callbacks finish before the
    // owning adapter handle/library are released.
    equalizer: Option<crate::effects::equalizer::EqualizerHost>,
    equalizer_library: Option<PathBuf>,
    sequence: u64,
    events: VecDeque<EngineEvent>,
    entry: Option<i64>,
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
    /// Attach the trusted native adapter to this player's actual decoder/filter
    /// chain. Opening before a track is allowed; stats distinguish configuration
    /// from actual PCM processing. No network or entitlement side effects.
    pub fn attach_equalizer(
        &mut self,
        library: &std::path::Path,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        if self.equalizer.is_some() {
            return Err(Error::State("equalizer is already attached"));
        }
        let host = crate::effects::equalizer::EqualizerHost::open(library, settings)?;
        let graph = host.filter_graph();
        let filter = format!("@cumulus-geq:lavfi=graph=%{}%{}", graph.len(), graph);
        self.control(&["af", "add", &filter])?;
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
            .update(settings)
    }
    pub fn equalizer_stats(&self) -> Result<crate::effects::equalizer::abi::EqualizerStats> {
        self.equalizer
            .as_ref()
            .ok_or(Error::State("equalizer adapter is not attached"))?
            .stats()
    }
    pub fn detach_equalizer(&mut self) -> Result<()> {
        if self.equalizer.is_some() {
            self.control(&["af", "remove", "@cumulus-geq"])?;
            self.equalizer = None;
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
        let frames = engine.native.start_video(width, height)?;
        Ok((engine, frames))
    }
    fn open_mode(config: MpvConfig, video: bool) -> Result<Self> {
        let native = if let Some(path) = config.library {
            Native::open(path.as_os_str())?
        } else {
            let names: &[&str] = if cfg!(target_os = "windows") {
                &["libmpv-2.dll", "mpv-2.dll"]
            } else if cfg!(target_os = "macos") {
                &["libmpv.2.dylib", "libmpv.dylib"]
            } else {
                &["libmpv.so.2", "libmpv.so.1"]
            };
            names
                .iter()
                .find_map(|name| Native::open(OsStr::new(name)).ok())
                .ok_or(Error::Playback {
                    operation: "library unavailable",
                    code: -1,
                })?
        };
        let mut engine = Self {
            native,
            equalizer: None,
            equalizer_library: None,
            sequence: 0,
            events: VecDeque::new(),
            entry: None,
        };
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
            engine.native.option(&string(key)?, &string(value)?)?;
        }
        // An unset ao selects the operating-system default. "auto" is not a
        // driver in the tested libmpv build (audio initialization error -14).
        if config.null_output {
            engine.native.option(c"ao", c"null")?;
        }
        engine.native.initialize()?;
        Ok(engine)
    }
    pub fn api_version(&self) -> u64 {
        self.native.version()
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
        self.native.command(sequence, &args)?;
        while Instant::now() < deadline {
            cancel.check()?;
            match self.native.event(0.05)? {
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
        self.command(
            args.iter().map(string).collect::<Result<Vec<_>>>()?,
            Instant::now() + Duration::from_secs(5),
            &Cancellation::default(),
        )
    }
    fn property(&mut self, name: &CStr) -> Option<String> {
        self.native.property(name)
    }
    fn number(&mut self, name: &CStr) -> Option<f64> {
        self.property(name)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
    fn drain(&mut self) -> Result<Vec<EngineEvent>> {
        for _ in 0..2048 {
            match self.native.event(0.0)? {
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
    fn load_inner(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        request.validate()?;
        cancel.check()?;
        let source = Self::source(request.source)?;
        let deadline = Instant::now() + request.timeout;
        self.command(vec![string("stop")?], deadline, cancel)?;
        self.entry = None;
        self.drain()?;
        for (key, value) in [
            ("pause", "yes".into()),
            ("options/start", request.start.to_string()),
            (
                "options/end",
                request.end.map(|n| n.to_string()).unwrap_or("none".into()),
            ),
        ] {
            self.command(
                vec![string("set")?, string(key)?, string(value)?],
                deadline,
                cancel,
            )?;
        }
        self.command(
            vec![string("loadfile")?, source, string("replace")?],
            deadline,
            cancel,
        )?;
        let mut entry = None;
        while Instant::now() < deadline {
            cancel.check()?;
            let event = if let Some(event) = self.events.pop_front() {
                Some(event)
            } else {
                match self.native.event(0.05)? {
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
    fn apply_equalizer(
        &mut self,
        settings: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<bool> {
        let Some(path) = self.equalizer_library.clone() else {
            return Ok(false);
        };
        if !settings.enabled()
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
            self.set_equalizer(settings)?;
        } else if settings.enabled() {
            self.attach_equalizer(&path, settings)?;
        }
        Ok(true)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        self.detach_equalizer()
    }
    fn observe_equalizer(
        &mut self,
    ) -> Result<Option<crate::effects::equalizer::abi::EqualizerStats>> {
        self.equalizer.as_ref().map(|h| h.stats()).transpose()
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let result = self.load_inner(request, cancel);
        if result.is_err() {
            let _ = self.stop();
        }
        result
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.control(&["set", "pause", if paused { "yes" } else { "no" }])
    }
    fn seek(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(Error::InvalidInput("invalid audio seek"));
        }
        self.control(&["seek", &seconds.to_string(), "absolute+exact"])
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
        self.control(&["set", "speed", &value.to_string()])
    }
    fn stop(&mut self) -> Result<()> {
        self.control(&["stop"])?;
        self.entry = None;
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        if let Some(equalizer) = &self.equalizer
            && equalizer.stats()?.failed_blocks != 0
        {
            // Failed DSP blocks are silenced at the callback boundary. Surface
            // the failure so the controller stops instead of showing normal
            // playback. Explicit detach/reattach creates a fresh error scope.
            return Err(Error::Media(
                "equalizer processing failed; recreate the adapter",
            ));
        }
        let events = self.drain()?;
        Ok(EngineSnapshot {
            entry_id: self.entry,
            position: self.number(c"time-pos"),
            duration: self.number(c"duration"),
            paused: self.property(c"pause").as_deref() == Some("yes"),
            buffering: self.property(c"paused-for-cache").as_deref() == Some("yes"),
            volume: self.number(c"volume"),
            speed: self.number(c"speed"),
            audio_output: self.property(c"current-ao"),
            events,
        })
    }
}
