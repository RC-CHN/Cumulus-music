//! Opt-in local SoXR host shared by the renderer and frozen proof.
use cumulus_core::{
    Error, Result,
    effects::dsp::{RotationRateConversion, RotationRateConverter},
};
use libloading::Library;
use std::{
    ffi::{CStr, c_char, c_ulong, c_void},
    path::Path,
    sync::Arc,
};

// This adapter is for explicit local tooling and proofs, not desktop discovery.
#[repr(C)]
struct Io {
    input: i32,
    output: i32,
    scale: f64,
    engine: *const c_void,
    flags: c_ulong,
}
type Create = unsafe extern "C" fn(
    f64,
    f64,
    u32,
    *mut *const c_char,
    *const Io,
    *const c_void,
    *const c_void,
) -> *mut c_void;
type Process = unsafe extern "C" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut usize,
    *mut c_void,
    usize,
    *mut usize,
) -> *const c_char;
type Clear = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type Delete = unsafe extern "C" fn(*mut c_void);
pub struct Host {
    _library: Library,
    create: Create,
    process: Process,
    clear: Clear,
    delete: Delete,
}
impl Host {
    /// Load only an explicitly selected local library.
    ///
    /// # Safety
    /// The caller must trust the named library and its Linux SoXR C ABI.
    pub unsafe fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() || !path.is_file() {
            return Err(Error::InvalidInput(
                "SoXR requires an absolute trusted library path",
            ));
        }
        if std::env::vars_os().any(|(key, _)| key.to_string_lossy().starts_with("SOXR_")) {
            return Err(Error::InvalidInput(
                "SoXR environment overrides are unsupported",
            ));
        }
        // SAFETY: explicit caller trust; keep the library alive for all symbols and handles.
        unsafe {
            let library =
                Library::new(path).map_err(|_| Error::Media("cannot open trusted SoXR library"))?;
            let invalid = |_| Error::Media("invalid SoXR library exports");
            let version = library
                .get::<unsafe extern "C" fn() -> *const c_char>(b"soxr_version\0")
                .map_err(invalid)?();
            if version.is_null() || CStr::from_ptr(version).to_bytes() != b"libsoxr-0.1.3" {
                return Err(Error::Media("unsupported SoXR version"));
            }
            Ok(Self {
                create: *library.get(b"soxr_create\0").map_err(invalid)?,
                process: *library.get(b"soxr_process\0").map_err(invalid)?,
                clear: *library.get(b"soxr_clear\0").map_err(invalid)?,
                delete: *library.get(b"soxr_delete\0").map_err(invalid)?,
                _library: library,
            })
        }
    }
}

pub struct Converter {
    host: Arc<Host>,
    handle: *mut c_void,
}
impl Converter {
    pub fn new(host: Arc<Host>, rate: RotationRateConversion) -> Result<Self> {
        let spec = Io {
            input: 4,
            output: 4,
            scale: 1.0,
            engine: std::ptr::null(),
            flags: 0,
        };
        let mut error = std::ptr::null();
        // SAFETY: parameters and output error pointer follow the pinned ABI.
        let handle = unsafe {
            (host.create)(
                f64::from(rate.input_rate()),
                f64::from(rate.output_rate()),
                2,
                &mut error,
                &spec,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if handle.is_null() {
            return Err(Error::Media("SoXR initialization failed"));
        }
        let converter = Self { host, handle };
        if !error.is_null() {
            return Err(Error::Media("SoXR initialization failed"));
        }
        Ok(converter)
    }
}
impl RotationRateConverter for Converter {
    fn process(&mut self, input: Option<[&[f32]; 2]>, [l, r]: [&mut [f32]; 2]) -> Result<usize> {
        let (samples, frames) = if let Some([l, r]) = input {
            if l.len() != r.len() {
                return Err(Error::InvalidInput(
                    "SoXR input planes must have equal lengths",
                ));
            }
            (Some([l.as_ptr(), r.as_ptr()]), l.len())
        } else {
            (None, 0)
        };
        if l.len() != r.len() {
            return Err(Error::InvalidInput(
                "SoXR output planes must have equal lengths",
            ));
        }
        let mut out = [l.as_mut_ptr(), r.as_mut_ptr()];
        let mut done = 0;
        // SAFETY: both split planes span the lengths supplied and the
        // exclusive converter stays live through this synchronous call.
        let error = unsafe {
            (self.host.process)(
                self.handle,
                samples
                    .as_ref()
                    .map_or(std::ptr::null(), |p| p.as_ptr().cast()),
                frames,
                std::ptr::null_mut(),
                out.as_mut_ptr().cast(),
                l.len(),
                &mut done,
            )
        };
        if !error.is_null() {
            return Err(Error::Media("SoXR conversion failed"));
        }
        Ok(done)
    }
    fn clear(&mut self) -> Result<()> {
        // SAFETY: live exclusively owned native converter.
        if unsafe { (self.host.clear)(self.handle) }.is_null() {
            Ok(())
        } else {
            Err(Error::Media("SoXR clear failed"))
        }
    }
}
impl Drop for Converter {
    fn drop(&mut self) {
        // SAFETY: owned live handle; host library retained until after this call.
        unsafe { (self.host.delete)(self.handle) };
    }
}
