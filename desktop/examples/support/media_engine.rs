//! Observe protocol requests at the real native engine boundary.
//! This example records successful seeks without replacing or adjusting audio.
use cumulus_core::{Result, playback::engine::*};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
pub struct LoadDelay {
    armed: AtomicBool,
    count: AtomicUsize,
}
impl LoadDelay {
    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }
}
pub type Probe = Arc<Mutex<Vec<f64>>>;
pub struct Observed {
    inner: crate::relay_fixture::NativeAudio,
    probe: Probe,
    delay: Option<Arc<LoadDelay>>,
}
impl Observed {
    pub fn new(inner: crate::relay_fixture::NativeAudio, probe: Probe) -> Self {
        Self {
            inner,
            probe,
            delay: None,
        }
    }
    pub fn with_load_delay(mut self, delay: Arc<LoadDelay>) -> Self {
        self.delay = Some(delay);
        self
    }
}
impl AudioEngine for Observed {
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        if let Some(delay) = &self.delay
            && delay.armed.swap(false, Ordering::AcqRel)
        {
            delay.count.fetch_add(1, Ordering::AcqRel);
            for _ in 0..90 {
                c.check()?;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
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
