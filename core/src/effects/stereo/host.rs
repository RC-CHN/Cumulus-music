use super::{StereoSettings, abi};
use crate::{
    Error, Result,
    effects::dsp::{AmbienceOneResult, Format, PlanarBlock},
};
use libloading::Library;
use std::{
    cell::Cell,
    ffi::c_void,
    marker::PhantomData,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

struct Adapter {
    _library: Library,
    _path: PathBuf,
    update: abi::Update,
    stats: abi::Stats,
    release: abi::Release,
    instantiate: abi::Instantiate,
    reset: abi::Reset,
    process: abi::Process,
    destroy: abi::Destroy,
}
pub struct StereoHost {
    adapter: Arc<Adapter>,
    id: u32,
}
pub struct StereoInstance {
    adapter: Arc<Adapter>,
    handle: NonNull<c_void>,
    format: Format,
    _not_sync: PhantomData<Cell<()>>,
}
// SAFETY: the private native instance owns Send DSP state without thread
// affinity; all callbacks require &mut, and the library stays owned by Arc.
unsafe impl Send for StereoInstance {}
fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Playback {
            operation: "stereo component adapter",
            code,
        })
    }
}
impl StereoHost {
    /// Path comes only from trusted native setup. This grants no material or
    /// source entitlement, and does not attach an incomplete LADSPA graph.
    pub fn open(path: &Path, settings: StereoSettings) -> Result<Self> {
        let path = path.canonicalize()?;
        if !path.is_file() || path.to_str().is_none() {
            return Err(Error::InvalidInput(
                "stereo adapter requires a regular UTF-8 path",
            ));
        }
        // SAFETY: native-selected trusted library, version and exact symbols
        // checked before use; owned Arc outlives every copied function pointer.
        unsafe {
            let library =
                Library::new(&path).map_err(|_| Error::State("stereo adapter unavailable"))?;
            let version = *library
                .get::<abi::Version>(b"cumulus_se1_version\0")
                .map_err(|_| Error::State("stereo adapter ABI missing"))?;
            if version() != abi::VERSION {
                return Err(Error::State("unsupported stereo adapter ABI"));
            }
            let create = *library
                .get::<abi::Create>(b"cumulus_se1_create_v1\0")
                .map_err(|_| Error::State("stereo adapter ABI missing"))?;
            let adapter = Adapter {
                update: *library
                    .get::<abi::Update>(b"cumulus_se1_update_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                stats: *library
                    .get::<abi::Stats>(b"cumulus_se1_stats_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                release: *library
                    .get::<abi::Release>(b"cumulus_se1_release_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                instantiate: *library
                    .get::<abi::Instantiate>(b"cumulus_se1_instantiate_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                reset: *library
                    .get::<abi::Reset>(b"cumulus_se1_reset_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                process: *library
                    .get::<abi::Process>(b"cumulus_se1_process_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                destroy: *library
                    .get::<abi::Destroy>(b"cumulus_se1_destroy_v1\0")
                    .map_err(|_| Error::State("stereo adapter ABI missing"))?,
                _library: library,
                _path: path,
            };
            let c = settings.controls();
            let mut id = 0;
            status(create(
                c.presence,
                c.stereoizer,
                c.spectrum_shaper.into(),
                settings.enabled().into(),
                &mut id,
            ))?;
            if !(1..=abi::MAX_ID).contains(&id) {
                return Err(Error::State("invalid stereo configuration ID"));
            }
            Ok(Self {
                adapter: Arc::new(adapter),
                id,
            })
        }
    }
    pub fn update(&mut self, settings: StereoSettings) -> Result<()> {
        let c = settings.controls();
        // SAFETY: live owned ID and validated scalar tuple, no borrowed pointers.
        status(unsafe {
            (self.adapter.update)(
                self.id,
                c.presence,
                c.stereoizer,
                c.spectrum_shaper.into(),
                settings.enabled().into(),
            )
        })
    }
    pub fn stats(&self) -> Result<abi::StereoStats> {
        let mut stats = abi::StereoStats::default();
        // SAFETY: exact writable version-checked layout, no retained reference.
        status(unsafe { (self.adapter.stats)(self.id, &mut stats) })?;
        Ok(stats)
    }
    /// Caller must pass the actual decoder's channel count, not a forced
    /// output-device layout. Unknown/mono/multichannel cannot be coerced here.
    pub fn instantiate(&self, sample_rate: u32, decoded_channels: u32) -> Result<StereoInstance> {
        if decoded_channels != 2 {
            return Err(Error::InvalidInput("SE1 requires proven decoded stereo"));
        }
        let format = Format::new(sample_rate as f32, 2, abi::CAPACITY)?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: live ID, bounded format, exact writable output pointer.
        status(unsafe { (self.adapter.instantiate)(self.id, sample_rate, &mut raw) })?;
        let handle = NonNull::new(raw).ok_or(Error::State("stereo native instance missing"))?;
        Ok(StereoInstance {
            adapter: self.adapter.clone(),
            handle,
            format,
            _not_sync: PhantomData,
        })
    }
}
impl Drop for StereoHost {
    fn drop(&mut self) {
        // SAFETY: ID released once. Any retained instance observes revocation;
        // its Arc keeps code loaded until its own acknowledged destruction.
        let _ = unsafe { (self.adapter.release)(self.id) };
    }
}
impl StereoInstance {
    pub fn format(&self) -> Format {
        self.format
    }
    pub fn reset(&mut self) -> Result<()> {
        // SAFETY: exclusive live instance; original reset, not recreation.
        status(unsafe { (self.adapter.reset)(self.handle.as_ptr()) })
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|x| !x.is_finite())
        {
            return Err(Error::InvalidInput("invalid stereo host PCM block"));
        }
        let mut result = abi::BlockResult::default();
        // SAFETY: complete initialized exclusive 2*CAPACITY planar storage,
        // even for zero frames; synchronous callback does not retain it.
        let status = status(unsafe {
            (self.adapter.process)(
                self.handle.as_ptr(),
                block.samples_mut().as_mut_ptr(),
                frames as u32,
                end.into(),
                &mut result,
            )
        });
        if status.is_err()
            || result.frames as usize > self.format.capacity()
            || result.end > 1
            || block.samples().iter().any(|x| !x.is_finite())
        {
            block.samples_mut().fill(0.0);
            return Err(Error::Media("stereo native host failed"));
        }
        Ok(AmbienceOneResult {
            valid_frames: result.frames as usize,
            end: result.end != 0,
        })
    }
}
impl Drop for StereoInstance {
    fn drop(&mut self) {
        // SAFETY: sole live handle, no outstanding &mut callback; code stays
        // loaded until after this synchronous destructor finishes.
        unsafe { (self.adapter.destroy)(self.handle.as_ptr()) };
    }
}
