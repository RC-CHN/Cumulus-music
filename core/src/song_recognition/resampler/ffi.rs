use super::loader::LibraryPair;
use crate::{Error, Result};
use libloading::Library;
use std::{
    ffi::c_void,
    path::Path,
    ptr::{self, NonNull},
};

const AVUTIL_VERSION: u32 = 0x0038_3364;
const SWRESAMPLE_VERSION: u32 = 0x0003_0764;
pub(super) const MONO: i64 = 4;
pub(super) const STEREO: i64 = 3;

type Version = unsafe extern "C" fn() -> u32;
type Allocate = unsafe extern "C" fn(
    *mut c_void,
    i64,
    i32,
    i32,
    i64,
    i32,
    i32,
    i32,
    *mut c_void,
) -> *mut c_void;
type Init = unsafe extern "C" fn(*mut c_void) -> i32;
type Delay = unsafe extern "C" fn(*mut c_void, i64) -> i64;
type Convert = unsafe extern "C" fn(*mut c_void, *mut *mut u8, i32, *const *const u8, i32) -> i32;
type Free = unsafe extern "C" fn(*mut *mut c_void);

pub(super) struct Api {
    pub allocate: Allocate,
    pub init: Init,
    pub delay: Delay,
    pub convert: Convert,
    pub free: Free,
    _libraries: LibraryPair,
}

impl Api {
    pub(super) unsafe fn load(directory: &Path) -> Result<Self> {
        // SAFETY: caller vouches for native libraries; exact ABIs checked below.
        let pair = unsafe { LibraryPair::open(directory)? };
        // SAFETY: trusted avutil/swresample export their documented version functions.
        let av = unsafe { function::<Version>(&pair.avutil, b"avutil_version\0")? };
        let swr = unsafe { function::<Version>(&pair.swresample, b"swresample_version\0")? };
        if unsafe { av() != AVUTIL_VERSION || swr() != SWRESAMPLE_VERSION } {
            return Err(Error::State(
                "recognition resampler ABI version is unsupported",
            ));
        }
        let library = &pair.swresample;
        // SAFETY: verified library pair and exact version, signatures match the
        // C exports. Keep both libraries loaded for every function pointer.
        unsafe {
            Ok(Self {
                allocate: function(library, b"swr_alloc_set_opts\0")?,
                init: function(library, b"swr_init\0")?,
                delay: function(library, b"swr_get_delay\0")?,
                convert: function(library, b"swr_convert\0")?,
                free: function(library, b"swr_free\0")?,
                _libraries: pair,
            })
        }
    }
}

unsafe fn function<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
    // SAFETY: callers specify the matching export's function pointer type.
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|_| Error::State("recognition resampler ABI symbol unavailable"))
}

#[derive(Clone, Copy)]
pub(super) struct Format {
    pub layout: i64,
    pub sample: i32,
    pub rate: i32,
}

pub(super) struct Context<'a> {
    api: &'a Api,
    native: NonNull<c_void>,
    pub input: Format,
    pub output: Format,
}

impl<'a> Context<'a> {
    pub(super) fn new(api: &'a Api, input: Format, output: Format) -> Result<Self> {
        // SAFETY: allocate a fresh context from fixed, valid stock formats.
        let pointer = unsafe {
            (api.allocate)(
                ptr::null_mut(),
                output.layout,
                output.sample,
                output.rate,
                input.layout,
                input.sample,
                input.rate,
                0,
                ptr::null_mut(),
            )
        };
        let native =
            NonNull::new(pointer).ok_or(Error::State("recognition resampler allocation failed"))?;
        let this = Self {
            api,
            native,
            input,
            output,
        };
        // SAFETY: this owns the allocated context, even if initialization fails.
        if unsafe { (api.init)(this.native.as_ptr()) } != 0 {
            return Err(Error::State("recognition resampler initialization failed"));
        }
        Ok(this)
    }

    pub(super) fn delay(&self) -> Result<i64> {
        // SAFETY: context is live and initialized; rate is positive.
        let delay = unsafe { (self.api.delay)(self.native.as_ptr(), i64::from(self.input.rate)) };
        if !(0..=i64::from(self.input.rate)).contains(&delay) {
            return Err(Error::State(
                "recognition resampler delay is outside its bound",
            ));
        }
        Ok(delay)
    }

    pub(super) unsafe fn convert(
        &mut self,
        output: *mut u8,
        output_capacity: i32,
        input: Option<*const u8>,
        frames: i32,
    ) -> Result<usize> {
        let mut out_planes = [output];
        let in_planes = [input.unwrap_or(ptr::null())];
        // SAFETY: internal caller provides the buffers and counts for these
        // fixed packed formats. libswresample does not retain caller pointers.
        let produced = unsafe {
            (self.api.convert)(
                self.native.as_ptr(),
                out_planes.as_mut_ptr(),
                output_capacity,
                if input.is_some() {
                    in_planes.as_ptr()
                } else {
                    ptr::null()
                },
                frames,
            )
        };
        if produced < 0 || produced > output_capacity {
            return Err(Error::State("recognition resampler conversion failed"));
        }
        Ok(produced as usize)
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        let mut pointer = self.native.as_ptr();
        // SAFETY: this uniquely owns the context; API and libraries outlive it.
        unsafe { (self.api.free)(&mut pointer) };
    }
}
