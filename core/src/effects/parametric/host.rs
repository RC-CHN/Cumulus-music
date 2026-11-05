use super::{ABI_VERSION, ParametricPlan, PeqOperation};
use crate::{Error, Result, effects::equalizer::abi};
use libloading::Library;
use std::path::{Path, PathBuf};

type Bypass = unsafe extern "C" fn(u32) -> i32;
type Restore = unsafe extern "C" fn(u32, *const PeqOperation, u32) -> i32;
type Create = unsafe extern "C" fn(*const PeqOperation, u32, *mut u32) -> i32;

/// Owns one immutable material component, independent of GEQ/loudness.
pub struct ParametricHost {
    _library: Library,
    path: PathBuf,
    id: u32,
    stats: abi::Stats,
    bypass: Option<Bypass>,
    restore: Option<Restore>,
    release: abi::Release,
}
fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Playback {
            operation: "parametric equalizer adapter",
            code,
        })
    }
}

impl ParametricHost {
    /// Trusted native library chosen by the host. No renderer paths or URLs.
    pub fn open(path: &Path, plan: ParametricPlan) -> Result<Self> {
        let path = path.canonicalize()?;
        if !path.is_file() || path.to_str().is_none() {
            return Err(Error::InvalidInput(
                "PEQ library requires a regular UTF-8 path",
            ));
        }
        // SAFETY: trusted library selected by native host, retained for ID lifetime.
        unsafe {
            let library =
                Library::new(&path).map_err(|_| Error::State("PEQ library unavailable"))?;
            let version = *library
                .get::<abi::Version>(b"cumulus_peq_version\0")
                .map_err(|_| Error::State("PEQ ABI missing"))?;
            if version() != ABI_VERSION {
                return Err(Error::State("unsupported PEQ ABI"));
            }
            let create = *library
                .get::<Create>(b"cumulus_peq_create_v1\0")
                .map_err(|_| Error::State("PEQ ABI missing"))?;
            let stats = *library
                .get::<abi::Stats>(b"cumulus_geq_stats_v1\0")
                .map_err(|_| Error::State("PEQ ABI missing"))?;
            let release = *library
                .get::<abi::Release>(b"cumulus_geq_release_v1\0")
                .map_err(|_| Error::State("PEQ ABI missing"))?;
            // Existing v1 libraries can still play an enabled PEQ component.
            let bypass = library
                .get::<Bypass>(b"cumulus_peq_bypass_v1\0")
                .ok()
                .map(|f| *f);
            let restore = library
                .get::<Restore>(b"cumulus_peq_restore_v1\0")
                .ok()
                .map(|f| *f);
            let (operations, count) = plan.operations();
            let mut id = 0;
            status(create(operations.as_ptr(), count, &mut id))?;
            if !(1..=abi::MAX_ID).contains(&id) {
                return Err(Error::State("invalid PEQ instance ID"));
            }
            Ok(Self {
                _library: library,
                path,
                id,
                stats,
                bypass,
                restore,
                release,
            })
        }
    }

    pub fn bypass(&self) -> Result<()> {
        let bypass = self.bypass.ok_or(Error::State("PEQ bypass ABI missing"))?;
        // SAFETY: optional symbol belongs to our retained, version-checked library.
        status(unsafe { bypass(self.id) })
    }

    pub fn restore(&self, plan: ParametricPlan) -> Result<()> {
        let restore = self
            .restore
            .ok_or(Error::State("PEQ restore ABI missing"))?;
        let (operations, count) = plan.operations();
        // SAFETY: owned bounded operations are read only while the retained
        // ABI-checked function runs.
        status(unsafe { restore(self.id, operations.as_ptr(), count) })
    }

    pub fn stats(&self) -> Result<abi::EqualizerStats> {
        let mut output = abi::EqualizerStats::default();
        // SAFETY: the ABI-checked shared stats function writes to owned storage.
        status(unsafe { (self.stats)(self.id, &mut output) })?;
        Ok(output)
    }

    pub fn filter_graph(&self) -> String {
        super::super::filter_graph::graph(&self.path, "cumulus_peq", abi::CAPACITY, self.id)
    }
}

impl Drop for ParametricHost {
    fn drop(&mut self) {
        // SAFETY: release the owned ID once before dropping its library.
        let _ = unsafe { (self.release)(self.id) };
    }
}
