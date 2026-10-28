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
    pub paused: bool,
    pub buffering: bool,
    pub volume: Option<f64>,
    pub speed: Option<f64>,
    pub audio_output: Option<String>,
    pub events: Vec<EngineEvent>,
}
