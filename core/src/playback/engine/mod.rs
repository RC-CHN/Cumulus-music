//! Synchronous native audio boundary. Hosts/controllers run it on an audio or
//! blocking worker, never the WebView thread. No shell or Python process.
mod bmff;
mod ffi;
mod flac_runtime;
mod frames;
mod limiter_segment;
mod mpv;
mod output;
mod runtime;
mod stereo_pcm;
mod stereo_transport;
mod types;
mod video;
use crate::Result;
pub use flac_runtime::TrustedFlacRuntime;
pub use frames::{VideoFrame, VideoFrames};
pub use limiter_segment::ColdLimiterWave;
pub(crate) use limiter_segment::{ColdLimiterConfig, ColdLimiterRender, ColdLimiterSource};
pub use limiter_segment::{ColdStereoPass, ColdStereoSelection, ColdStereoSource, ColdStereoStart};
pub(crate) use limiter_segment::{
    NativeStereoPass, NativeStereoSelection, NativeStereoSource, NativeStereoStart,
};
pub use mpv::{MpvConfig, MpvEngine};
pub use output::{OutputDevice, OutputDevices};
pub use runtime::MpvRuntime;
pub use stereo_pcm::{StereoPcmControl, StereoPcmReader, StereoPcmSnapshot};
pub use stereo_transport::{StereoGeneration, StereoRead, StereoSourceTransport};
pub use types::*;
pub use video::{MpvVideoEngine, VideoEngine};

/// Select a bounded audio-only demuxer from the native handle, without trusting
/// a filename extension or enabling playlist/container URL resolution.
pub(crate) fn pinned_audio_format(file: &mut std::fs::File) -> Result<&'static str> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let mut magic = [0_u8; 12];
    file.read_exact(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    match &magic {
        [b'f', b'L', b'a', b'C', ..] => Ok("flac"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E'] => Ok("wav"),
        [b'I', b'D', b'3', ..] => Ok("mp3"),
        [0xff, next, ..] if next & 0xe0 == 0xe0 && next & 0x06 != 0 => Ok("mp3"),
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => {
            bmff::validate(file)?;
            Ok("mov")
        }
        _ => Err(crate::Error::InvalidInput(
            "unsupported pinned audio format",
        )),
    }
}

pub(crate) fn pinned_ncm_format(
    reader: &mut crate::media::ncm::NcmReader<impl std::io::Read + std::io::Seek>,
) -> Result<&'static str> {
    use crate::media::ncm::AudioFormat;
    match reader.info().audio_format {
        Some(AudioFormat::Flac) => Ok("flac"),
        Some(AudioFormat::Mp3) => Ok("mp3"),
        Some(AudioFormat::M4a) => {
            // Validate against the decrypted payload's coordinates. The
            // underlying NCM header/cover cannot satisfy any BMFF offset.
            let length = reader.info().audio_bytes;
            bmff::validate_stream(reader, length)?;
            Ok("mov")
        }
        None => Err(crate::Error::InvalidInput(
            "unsupported pinned NCM audio format",
        )),
    }
}

// The private stream source can only be staged by the native engine.
trait PinnedReader: std::io::Read + std::io::Seek + Send {}
impl<T: std::io::Read + std::io::Seek + Send> PinnedReader for T {}

pub trait AudioEngine: Send {
    /// Observe the actual decoder format for this entry; unknown never implies
    /// stereo. Implementations must not report a forced output-device layout.
    fn decoded_audio_format(&mut self, _entry_id: i64) -> Result<Option<DecodedAudioFormat>> {
        Ok(None)
    }
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
    /// A source conflict must not detach/reinsert the ordinary EQ host while
    /// a decoder initializes. Unsupported engines cannot acknowledge it.
    fn bypass_equalizer_for_source(&mut self) -> Result<bool> {
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
    /// Called after stopping the previous source and before loading another.
    /// A healthy fixed-frame host may keep a dry node without the old gain.
    fn reset_track_loudness(&mut self) -> Result<()> {
        self.clear_loudness()
    }
    fn observe_loudness(&mut self) -> Result<Option<crate::effects::loudness::abi::LoudnessStats>> {
        Ok(None)
    }
    /// Explicit native PEQ component; false means the engine has no adapter.
    fn apply_parametric_component(
        &mut self,
        _: crate::effects::parametric::ParametricPlan,
    ) -> Result<bool> {
        Ok(false)
    }
    /// Run the existing PEQ component's native transition to dry.
    /// Engines without an attached component are already dry.
    fn bypass_parametric_component(&mut self) -> Result<()> {
        Ok(())
    }
    /// Restore the existing native component without replacing its graph node.
    fn restore_parametric_component(
        &mut self,
        _: crate::effects::parametric::ParametricPlan,
    ) -> Result<()> {
        Err(crate::Error::State("parametric restore is unavailable"))
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        Ok(())
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia>;
    fn supports_local_equalizer(&self) -> bool {
        false
    }
    /// A native, explicitly opened file can use the recovered combined EQ
    /// material. Backends without that decoder keep their ordinary load.
    fn load_with_local_equalizer(
        &mut self,
        request: LoadRequest,
        _: crate::effects::equalizer::EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        self.load(request, cancel)
    }
    /// Identifies the bound native entry, never a queue item or UI authority.
    fn local_equalizer_entry(&self) -> Option<i64> {
        None
    }
    fn pump_local_equalizer(&mut self) -> Result<()> {
        Ok(())
    }
    fn apply_local_equalizer(
        &mut self,
        _: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        Err(crate::Error::State("native local equalizer is unavailable"))
    }
    fn local_equalizer_processing(&mut self) -> Result<bool> {
        Ok(false)
    }
    /// An explicit request for an independent filter can leave the combined
    /// graph without reopening the file. Returns a new, paused stock entry.
    fn yield_local_equalizer(&mut self, _: i64, _: &Cancellation) -> Result<LoadedMedia> {
        Err(crate::Error::State("native local equalizer is unavailable"))
    }
    /// A native reader pilot can use only a verified read-only stream adapter.
    fn streaming_audio_available(&mut self) -> bool {
        false
    }
    fn load_streaming_audio(
        &mut self,
        _: super::StreamingAudioReader,
        _: &Cancellation,
    ) -> Result<LoadedMedia> {
        Err(crate::Error::State("native streaming audio is unavailable"))
    }
    /// Native-only, already-opened and verified local audio; default engines
    /// fail closed instead of reopening by path or using a URL.
    fn load_pinned_audio(
        &mut self,
        _: std::fs::File,
        _: Cancellation,
        _: &Cancellation,
    ) -> Result<LoadedMedia> {
        Err(crate::Error::State("pinned audio playback is unavailable"))
    }
    /// Native transport for verified NCM. The caller must check publication
    /// integrity and cached rights and include expiry in the authority.
    fn load_pinned_ncm(
        &mut self,
        _: std::fs::File,
        _: Cancellation,
        _: &Cancellation,
    ) -> Result<LoadedMedia> {
        Err(crate::Error::State("pinned NCM playback is unavailable"))
    }
    /// Optional native backend for the internally authorized cold limiter.
    /// Existing engines keep their ordinary pinned decoder.
    fn supports_cold_limiter(&self) -> bool {
        false
    }
    fn load_cold_limiter(&mut self, _: ColdLimiterWave, _: &Cancellation) -> Result<LoadedMedia> {
        Err(crate::Error::State("cold limiter playback is unavailable"))
    }
    fn load_cold_stock_flac(
        &mut self,
        _: crate::download::offline::ColdOfflineFlacReader,
        _: f64,
        _: &Cancellation,
    ) -> Result<LoadedMedia> {
        Err(crate::Error::State("cold FLAC handoff is unavailable"))
    }
    fn pause(&mut self, paused: bool) -> Result<()>;
    fn seek(&mut self, seconds: f64) -> Result<()>;
    /// A held source replaces the native decoder entry while retaining the
    /// logical play. Its caller re-applies pause intent through the outer host.
    fn seek_with_receipt(
        &mut self,
        seconds: f64,
        _: i64,
        _: &Cancellation,
    ) -> Result<Option<LoadedMedia>> {
        self.seek(seconds)?;
        Ok(None)
    }
    fn volume(&mut self, value: f64) -> Result<()>;
    fn speed(&mut self, value: f64) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn poll(&mut self) -> Result<EngineSnapshot>;
}
