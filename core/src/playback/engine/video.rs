use super::*;
use std::sync::Arc;
pub trait VideoEngine: AudioEngine {
    fn resize(&self, width: u32, height: u32) -> Result<()>;
    fn frame_after(&self, sequence: u64) -> Result<Option<Arc<VideoFrame>>>;
}
pub struct MpvVideoEngine {
    engine: MpvEngine,
    frames: VideoFrames,
}
impl MpvVideoEngine {
    pub fn open(config: MpvConfig, width: u32, height: u32) -> Result<Self> {
        let (engine, frames) = MpvEngine::open_video(config, width, height)?;
        Ok(Self { engine, frames })
    }
}
impl AudioEngine for MpvVideoEngine {
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        self.engine.output_devices()
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        self.engine.set_output_device(id)
    }
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        self.engine.load(r, c)
    }
    fn pause(&mut self, v: bool) -> Result<()> {
        self.engine.pause(v)
    }
    fn seek(&mut self, v: f64) -> Result<()> {
        self.engine.seek(v)
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        self.engine.volume(v)
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.engine.speed(v)
    }
    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.engine.poll()
    }
}
impl VideoEngine for MpvVideoEngine {
    fn resize(&self, w: u32, h: u32) -> Result<()> {
        self.frames.resize(w, h)
    }
    fn frame_after(&self, s: u64) -> Result<Option<Arc<VideoFrame>>> {
        self.frames.after(s)
    }
}
