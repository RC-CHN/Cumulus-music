//! Synthetic output failures/status bits over a real libmpv/null engine.
use super::relay_fixture::NativeAudio;
use cumulus_core::{
    Error, Result,
    playback::engine::{
        AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest, LoadedMedia,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub fn enabled() -> bool {
    std::env::var("CUMULUS_CARD_SOURCE_PROOF").as_deref() == Ok("1")
}
pub fn native_stop_enabled() -> bool {
    std::env::var("CUMULUS_PLAYBACK_NATIVE_STOP_PROOF").as_deref() == Ok("1")
}
static FIRST_SONG_AUTHORIZATIONS: AtomicU64 = AtomicU64::new(0);
pub fn record_authorization(ids: &[Value]) {
    if native_stop_enabled() && matches!(ids.first(), Some(id) if id == 1 || id == "1") {
        FIRST_SONG_AUTHORIZATIONS.fetch_add(1, Ordering::SeqCst);
    }
}
#[derive(Default)]
pub struct Control {
    buffer: AtomicBool,
    fail_resume: AtomicBool,
    fail_output: AtomicBool,
    loads: AtomicU64,
    failed_resumes: AtomicU64,
    request_native_stop: AtomicBool,
    observed_native_stops: AtomicU64,
}
impl Control {
    pub fn configure(&self, mode: &str) -> Result<Value> {
        match mode {
            "buffer-on" => self.buffer.store(true, Ordering::SeqCst),
            "buffer-off" => self.buffer.store(false, Ordering::SeqCst),
            "fail-next-resume" => self.fail_resume.store(true, Ordering::SeqCst),
            "fail-next-output" => self.fail_output.store(true, Ordering::SeqCst),
            "stop-in-native" if native_stop_enabled() => {
                self.request_native_stop.store(true, Ordering::SeqCst)
            }
            "read" => {}
            _ => return Err(Error::InvalidInput("unknown synthetic card audio mode")),
        }
        Ok(self.report())
    }
    pub fn report(&self) -> Value {
        let mut report = json!({"loads":self.loads.load(Ordering::SeqCst),"failedResumes":self.failed_resumes.load(Ordering::SeqCst),"forcedBuffering":self.buffer.load(Ordering::SeqCst),"pendingResumeFailure":self.fail_resume.load(Ordering::SeqCst),"pendingOutputFailure":self.fail_output.load(Ordering::SeqCst)});
        if native_stop_enabled() {
            report["observedNativeStops"] =
                json!(self.observed_native_stops.load(Ordering::SeqCst));
            report["songAuthorizations"] = json!(FIRST_SONG_AUTHORIZATIONS.load(Ordering::SeqCst));
        }
        report
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
        if self
            .control
            .request_native_stop
            .swap(false, Ordering::SeqCst)
        {
            // Issue libmpv's real command without a Stop request to the actor.
            // Its native EndReason::Stop is returned without editing it.
            self.inner.stop()?;
        }
        let mut snapshot = self.inner.poll()?;
        if native_stop_enabled() {
            let count = snapshot
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        EngineEvent::Ended {
                            reason: EndReason::Stop,
                            ..
                        }
                    )
                })
                .count();
            self.control
                .observed_native_stops
                .fetch_add(count as u64, Ordering::SeqCst);
        }
        if self.control.buffer.load(Ordering::SeqCst)
            && !snapshot.paused
            && snapshot.entry_id.is_some()
        {
            snapshot.buffering = true;
        }
        Ok(snapshot)
    }
}
