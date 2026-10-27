//! Synchronous native audio boundary. Hosts/controllers run it on an audio or
//! blocking worker, never the WebView thread. No shell or Python process.
mod ffi;
mod frames;
mod mpv;
mod types;
mod video;
use crate::Result;
pub use frames::{VideoFrame, VideoFrames};
pub use mpv::{MpvConfig, MpvEngine};
pub use types::*;
pub use video::{MpvVideoEngine, VideoEngine};

pub trait AudioEngine: Send {
    /// Return false when this engine has no effect adapter. Default preserves
    /// existing engines without pretending their PCM has been processed.
    fn apply_equalizer(&mut self, _: crate::effects::equalizer::EqualizerSettings) -> Result<bool> {
        Ok(false)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        Ok(())
    }
    fn observe_equalizer(
        &mut self,
    ) -> Result<Option<crate::effects::equalizer::abi::EqualizerStats>> {
        Ok(None)
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia>;
    fn pause(&mut self, paused: bool) -> Result<()>;
    fn seek(&mut self, seconds: f64) -> Result<()>;
    fn volume(&mut self, value: f64) -> Result<()>;
    fn speed(&mut self, value: f64) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn poll(&mut self) -> Result<EngineSnapshot>;
}
