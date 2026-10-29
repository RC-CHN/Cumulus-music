//! Synchronous native audio boundary. Hosts/controllers run it on an audio or
//! blocking worker, never the WebView thread. No shell or Python process.
mod ffi;
mod frames;
mod mpv;
mod output;
mod types;
mod video;
use crate::Result;
pub use frames::{VideoFrame, VideoFrames};
pub use mpv::{MpvConfig, MpvEngine};
pub use output::{OutputDevice, OutputDevices};
pub use types::*;
pub use video::{MpvVideoEngine, VideoEngine};

pub trait AudioEngine: Send {
    /// Device preference maintenance when no decoder load is holding the
    /// engine. Adapters throttle read-only discovery; plain engines do nothing.
    fn maintain_output(&mut self) -> Result<()> {
        Ok(())
    }
    /// None means unsupported (including an explicitly isolated null output).
    /// Hosts call this on a blocking worker, never the UI thread.
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        Ok(None)
    }
    /// Successful return acknowledges the requested configuration, not actual
    /// endpoint activation. A host must observe playback and handle failure.
    fn set_output_device(&mut self, _: &str) -> Result<()> {
        Err(crate::Error::State("audio output selection is unavailable"))
    }
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
    fn apply_loudness(&mut self, _: crate::effects::loudness::LoudnessSettings) -> Result<bool> {
        Ok(false)
    }
    fn clear_loudness(&mut self) -> Result<()> {
        Ok(())
    }
    fn observe_loudness(&mut self) -> Result<Option<crate::effects::loudness::abi::LoudnessStats>> {
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
