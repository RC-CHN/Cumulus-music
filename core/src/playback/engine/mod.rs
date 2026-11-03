//! Synchronous native audio boundary. Hosts/controllers run it on an audio or
//! blocking worker, never the WebView thread. No shell or Python process.
mod bmff;
mod ffi;
mod frames;
mod mpv;
mod output;
mod runtime;
mod types;
mod video;
use crate::Result;
pub use frames::{VideoFrame, VideoFrames};
pub use mpv::{MpvConfig, MpvEngine};
pub use output::{OutputDevice, OutputDevices};
pub use runtime::MpvRuntime;
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
    fn pause(&mut self, paused: bool) -> Result<()>;
    fn seek(&mut self, seconds: f64) -> Result<()>;
    fn volume(&mut self, value: f64) -> Result<()>;
    fn speed(&mut self, value: f64) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn poll(&mut self) -> Result<EngineSnapshot>;
}
