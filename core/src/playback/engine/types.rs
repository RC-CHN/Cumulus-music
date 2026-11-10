use crate::{Error, Result, download::transfer::MediaUrl};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Native-only source. Authorized URLs must not be serialized into UI state.
#[derive(Clone)]
pub enum MediaInput {
    File(PathBuf),
    /// A cold native open is revalidated against its already opened handle.
    NativeFile {
        path: PathBuf,
        fingerprint: crate::library::identity::FileFingerprint,
    },
    /// An explicitly admitted fixed local Windows file, never opened by path.
    #[cfg(windows)]
    WindowsNativeFile(Arc<crate::library::WindowsOpenedAudio>),
    /// Internal publication receipt. A generic path/URL load must reject it.
    SearchPublication(Arc<crate::download::offline::SearchPublication>),
    Url(MediaUrl),
}

#[derive(Clone)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    valid: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl Default for Cancellation {
    fn default() -> Self {
        Self::new(|| true)
    }
}
impl Cancellation {
    pub fn new(valid: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            valid: Arc::new(valid),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || !(self.valid)()
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Native ownership receipt for the exact currently installed held PCM WAV.
/// No path, descriptor, source URL or playback control is exposed.
pub struct NativeModernEffectSource {
    control: super::StereoPcmControl,
    entry: i64,
    graph_revision: u64,
    format: crate::effects::dsp::Format,
}

impl NativeModernEffectSource {
    pub(crate) fn capture(control: super::StereoPcmControl, entry: i64) -> Result<Option<Self>> {
        let Some(graph_revision) = control.modern_admission_revision(entry)? else {
            return Ok(None);
        };
        let format = crate::effects::dsp::Format::new(control.rate() as f32, 2, 2048)?;
        let receipt = Self {
            control,
            entry,
            graph_revision,
            format,
        };
        receipt.check()?;
        Ok(Some(receipt))
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.control.modern_admission_revision(self.entry)? != Some(self.graph_revision) {
            return Err(Error::StaleResponse);
        }
        self.control.validate_host()
    }

    pub(crate) fn format(&self) -> crate::effects::dsp::Format {
        self.format
    }

    #[cfg(test)]
    pub(crate) fn test_source(
        rate: u32,
        lifetime: Cancellation,
    ) -> Result<(Self, super::StereoPcmReader)> {
        let (reader, control, shared) =
            super::stereo_pcm::native_channel(rate, 0, 48_000, lifetime)?;
        control.publish_sink()?;
        control.bind_entry(7)?;
        // The protocol fixture models a known positive worker state; native
        // DSP and receipt invalidation are tested with an actual held WAV.
        shared.publish_observing_graph(&[0.25, -0.25], 1, false, true)?;
        let receipt =
            Self::capture(control, 7)?.ok_or(Error::State("missing synthetic admission"))?;
        Ok((receipt, reader))
    }
}

#[derive(Clone)]
pub struct LoadRequest {
    pub source: MediaInput,
    pub start: f64,
    pub end: Option<f64>,
    pub timeout: Duration,
}
impl LoadRequest {
    pub fn new(source: MediaInput) -> Self {
        Self {
            source,
            start: 0.0,
            end: None,
            timeout: Duration::from_secs(25),
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.start.is_finite()
            || self.start < 0.0
            || self
                .end
                .is_some_and(|end| !end.is_finite() || end <= self.start)
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(120)
        {
            return Err(Error::InvalidInput("invalid audio load bounds"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct LoadedMedia {
    pub entry_id: i64,
    pub duration: Option<f64>,
    pub codec: Option<String>,
}

/// Native decoder output, before output-device/channel filter conversions.
/// This is an observation of one exact media entry, not a rights receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedAudioFormat {
    pub entry_id: i64,
    pub sample_rate: u32,
    pub channels: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stop,
    Quit,
    Error,
    Redirect,
    Unknown(i32),
}
impl From<i32> for EndReason {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Eof,
            2 => Self::Stop,
            3 => Self::Quit,
            4 => Self::Error,
            5 => Self::Redirect,
            other => Self::Unknown(other),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    /// A known native audio-output failure was switched to default output.
    /// The owning controller must reload its existing selection once, with
    /// its account/generation guard and current transport position preserved.
    OutputReload {
        entry_id: i64,
    },
    Started {
        entry_id: i64,
    },
    Loaded,
    Ended {
        entry_id: i64,
        reason: EndReason,
        error: i32,
    },
    Shutdown,
}
#[derive(Clone, Debug)]
pub struct EngineSnapshot {
    pub entry_id: Option<i64>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub buffered_through: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    /// The clock may still describe the pre-seek decode position.
    pub seeking: bool,
    pub volume: Option<f64>,
    pub speed: Option<f64>,
    pub audio_output: Option<String>,
    /// Samples from a live PEQ instance which observed its current native revision.
    pub parametric_samples: Option<u64>,
    pub events: Vec<EngineEvent>,
}

impl EngineSnapshot {
    /// A pending native seek can still report the old clock for this entry.
    pub(crate) fn position_for(&self, entry_id: i64) -> Option<f64> {
        self.position
            .filter(|_| self.entry_id == Some(entry_id) && !self.seeking)
    }
}
