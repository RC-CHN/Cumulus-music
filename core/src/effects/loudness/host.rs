use super::{LoudnessSettings, abi};
use crate::{Error, Result};
use libloading::Library;
use std::path::{Path, PathBuf};

/// Native-owned trusted library and independent per-player configuration.
/// Audio callbacks must finish before dropping this host/library.
pub struct LoudnessHost {
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
            operation: "loudness adapter",
            code,
        })
    }
}
impl LoudnessHost {
    pub fn open(path: &Path, settings: LoudnessSettings) -> Result<Self> {
        let path = path.canonicalize()?;
        if !path.is_file() || path.to_str().is_none() {
            return Err(Error::InvalidInput(
                "loudness library requires a regular UTF-8 path",
            ));
        }
        // SAFETY: host-selected trusted native library; checked version and
        // fixed synchronous ABI. The library outlives every copied function.
        unsafe {
            let library =
                Library::new(&path).map_err(|_| Error::State("loudness library unavailable"))?;
            let version = *library
                .get::<abi::Version>(b"cumulus_loudness_version\0")
                .map_err(|_| Error::State("loudness ABI missing"))?;
            if version() != abi::VERSION {
                return Err(Error::State("unsupported loudness ABI"));
            }
            let create = *library
                .get::<abi::Create>(b"cumulus_loudness_create_v1\0")
                .map_err(|_| Error::State("loudness ABI missing"))?;
            let update = *library
                .get::<abi::Update>(b"cumulus_loudness_update_v1\0")
                .map_err(|_| Error::State("loudness ABI missing"))?;
            let stats = *library
                .get::<abi::Stats>(b"cumulus_loudness_stats_v1\0")
                .map_err(|_| Error::State("loudness ABI missing"))?;
            let release = *library
                .get::<abi::Release>(b"cumulus_loudness_release_v1\0")
                .map_err(|_| Error::State("loudness ABI missing"))?;
            let mut id = 0;
            status(create(
                settings.gain_db(),
                settings.enabled().into(),
                &mut id,
            ))?;
            if !(1..=abi::MAX_ID).contains(&id) {
                return Err(Error::State("invalid loudness configuration ID"));
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
    pub fn update(&mut self, settings: LoudnessSettings) -> Result<()> {
        // SAFETY: owned live handle and validated scalar settings, no pointers.
        status(unsafe { (self.update)(self.id, settings.gain_db(), settings.enabled().into()) })
    }
    pub fn stats(&self) -> Result<abi::LoudnessStats> {
        let mut result = abi::LoudnessStats::default();
        // SAFETY: writable version-matched storage, no retained reference.
        status(unsafe { (self.stats)(self.id, &mut result) })?;
        Ok(result)
    }
    pub fn filter_graph(&self) -> String {
        super::super::filter_graph::graph(&self.path, "cumulus_loudness", abi::CAPACITY, self.id)
    }
}
impl Drop for LoudnessHost {
    fn drop(&mut self) {
        // SAFETY: release the owned handle once, before unloading the library.
        let _ = unsafe { (self.release)(self.id) };
    }
}
