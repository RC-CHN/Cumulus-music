//! Observe protocol requests at the real native engine boundary.
//! This example records successful seeks without replacing or adjusting audio.
use cumulus_core::{Result, playback::engine::*};
use std::sync::{Arc, Mutex};
pub type Probe = Arc<Mutex<Vec<f64>>>;
pub struct Observed {
    inner: crate::relay_fixture::NativeAudio,
    probe: Probe,
}
impl Observed {
    pub fn new(inner: crate::relay_fixture::NativeAudio, probe: Probe) -> Self {
        Self { inner, probe }
    }
}
impl AudioEngine for Observed {
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        self.inner.load(r, c)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.inner.pause(paused)
    }
    fn seek(&mut self, v: f64) -> Result<()> {
        self.inner.seek(v)?;
        let mut trace = self.probe.lock().unwrap();
        assert!(trace.len() < 256, "bounded synthetic seek trace");
        trace.push(v);
        Ok(())
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        self.inner.volume(v)
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.inner.speed(v)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
}
