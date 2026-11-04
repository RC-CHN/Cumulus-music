//! Synthetic output failures/status bits over a real libmpv/null engine.
use super::relay_fixture::NativeAudio;
use cumulus_core::{
    Error, Result,
    playback::engine::{AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub fn enabled() -> bool {
    std::env::var("CUMULUS_CARD_SOURCE_PROOF").as_deref() == Ok("1")
}
#[derive(Default)]
pub struct Control {
    buffer: AtomicBool,
    fail_resume: AtomicBool,
    fail_output: AtomicBool,
    loads: AtomicU64,
    failed_resumes: AtomicU64,
}
impl Control {
    pub fn configure(&self, mode: &str) -> Result<Value> {
        match mode {
            "buffer-on" => self.buffer.store(true, Ordering::SeqCst),
            "buffer-off" => self.buffer.store(false, Ordering::SeqCst),
            "fail-next-resume" => self.fail_resume.store(true, Ordering::SeqCst),
            "fail-next-output" => self.fail_output.store(true, Ordering::SeqCst),
            "read" => {}
            _ => return Err(Error::InvalidInput("unknown synthetic card audio mode")),
        }
        Ok(self.report())
    }
    pub fn report(&self) -> Value {
        json!({"loads":self.loads.load(Ordering::SeqCst),"failedResumes":self.failed_resumes.load(Ordering::SeqCst),"forcedBuffering":self.buffer.load(Ordering::SeqCst),"pendingResumeFailure":self.fail_resume.load(Ordering::SeqCst),"pendingOutputFailure":self.fail_output.load(Ordering::SeqCst)})
    }
}
pub struct Audio {
    inner: NativeAudio,
    control: Arc<Control>,
}
impl Audio {
    pub fn new(inner: NativeAudio, control: Arc<Control>) -> Self {
        Self { inner, control }
    }
}
impl AudioEngine for Audio {
    fn load(&mut self, input: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.control.loads.fetch_add(1, Ordering::SeqCst);
        if self.control.fail_output.swap(false, Ordering::SeqCst) {
            return Err(Error::Playback {
                operation: "load ended",
                code: -14,
            });
        }
        self.inner.load(input, cancel)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        if !paused && self.control.fail_resume.swap(false, Ordering::SeqCst) {
            self.control.failed_resumes.fetch_add(1, Ordering::SeqCst);
            return Err(Error::State("synthetic native resume failure"));
        }
        self.inner.pause(paused)
    }
    fn seek(&mut self, position: f64) -> Result<()> {
        self.inner.seek(position)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.inner.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.inner.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let mut snapshot = self.inner.poll()?;
        if self.control.buffer.load(Ordering::SeqCst)
            && !snapshot.paused
            && snapshot.entry_id.is_some()
        {
            snapshot.buffering = true;
        }
        Ok(snapshot)
    }
}
