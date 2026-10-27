use super::{EqualizerSettings, abi};
use crate::{Error, Result};
use libloading::Library;
use std::path::{Path, PathBuf};

/// Owns one configuration, isolated from other players/accounts. The library
/// path is selected by the native host, never accepted from an IPC settings DTO.
pub struct EqualizerHost {
    _library: Library,
    path: PathBuf,
    id: u32,
    update: abi::Update,
    stats: abi::Stats,
    release: abi::Release,
}
fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Playback {
            operation: "equalizer adapter",
            code,
        })
    }
}
impl EqualizerHost {
    pub fn open(path: &Path, settings: EqualizerSettings) -> Result<Self> {
        let path = path.canonicalize()?;
        if !path.is_file() || path.to_str().is_none() {
            return Err(Error::InvalidInput(
                "equalizer library requires a regular UTF-8 path",
            ));
        }
        // SAFETY: caller selects a trusted native library, as with libmpv. Its
        // symbols implement our fixed ABI. Library remains owned until Drop.
        unsafe {
            let library =
                Library::new(&path).map_err(|_| Error::State("equalizer library unavailable"))?;
            let version = *library
                .get::<abi::Version>(b"cumulus_geq_version\0")
                .map_err(|_| Error::State("equalizer ABI missing"))?;
            if version() != abi::VERSION {
                return Err(Error::State("unsupported equalizer ABI"));
            }
            let create = *library
                .get::<abi::Create>(b"cumulus_geq_create_v1\0")
                .map_err(|_| Error::State("equalizer ABI missing"))?;
            let update = *library
                .get::<abi::Update>(b"cumulus_geq_update_v1\0")
                .map_err(|_| Error::State("equalizer ABI missing"))?;
            let stats = *library
                .get::<abi::Stats>(b"cumulus_geq_stats_v1\0")
                .map_err(|_| Error::State("equalizer ABI missing"))?;
            let release = *library
                .get::<abi::Release>(b"cumulus_geq_release_v1\0")
                .map_err(|_| Error::State("equalizer ABI missing"))?;
            let mut id = 0;
            status(create(
                settings.gains().as_ptr(),
                settings.enabled().into(),
                &mut id,
            ))?;
            if !(1..=abi::MAX_ID).contains(&id) {
                return Err(Error::State("invalid equalizer instance ID"));
            }
            Ok(Self {
                _library: library,
                path,
                id,
                update,
                stats,
                release,
            })
        }
    }
    pub fn update(&mut self, settings: EqualizerSettings) -> Result<()> {
        // SAFETY: the owned ID is live, input is exactly ten readable floats.
        status(unsafe {
            (self.update)(
                self.id,
                settings.gains().as_ptr(),
                settings.enabled().into(),
            )
        })
    }
    pub fn stats(&self) -> Result<abi::EqualizerStats> {
        let mut result = abi::EqualizerStats::default();
        // SAFETY: result is writable and matches the version-checked C layout.
        status(unsafe { (self.stats)(self.id, &mut result) })?;
        Ok(result)
    }
    /// FFmpeg filter graph only; mpv adds its own length-prefixed quoting layer.
    pub fn filter_graph(&self) -> String {
        // Two tokenizers: option values first, then the outer filter graph.
        fn escape(value: &str, special: &str) -> String {
            let mut result = String::new();
            for c in value.chars() {
                if special.contains(c) {
                    result.push('\\');
                }
                result.push(c);
            }
            result
        }
        let value = escape(self.path.to_str().expect("checked path"), "\\': ");
        let value = escape(&value, "\\'[],; ");
        // The recovered four-sample recurrence has distinct scalar tail
        // rounding. Normalize decoder-dependent frame sizes before LADSPA,
        // also keeping bypass fades at our declared adapter capacity. Do not
        // pad EOF: preserve exact duration and the final partial block.
        format!(
            "asetnsamples=n={}:p=0,ladspa=file={value}:plugin=cumulus_geq:controls={}",
            abi::CAPACITY,
            self.id
        )
    }
}
impl Drop for EqualizerHost {
    fn drop(&mut self) {
        // SAFETY: owned ID is released once, before the library is unloaded.
        // Existing plugin instances hold their own Arc and become muted.
        let _ = unsafe { (self.release)(self.id) };
    }
}
