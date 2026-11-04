//! Pinned public libFLAC 1.5.0 ABI; no private decoder layout is used.
use libloading::Library;
#[cfg(test)]
use std::{ffi::CString, ptr::NonNull};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    io,
    path::Path,
};

pub(super) type Handle = *mut c_void;
pub(super) type Read = unsafe extern "C" fn(Handle, *mut u8, *mut usize, Handle) -> c_int;
pub(super) type Seek = unsafe extern "C" fn(Handle, u64, Handle) -> c_int;
pub(super) type Tell = unsafe extern "C" fn(Handle, *mut u64, Handle) -> c_int;
pub(super) type Eof = unsafe extern "C" fn(Handle, Handle) -> c_int;
pub(super) type Write =
    unsafe extern "C" fn(Handle, *const FrameHeader, *const *const i32, Handle) -> c_int;
type Metadata = unsafe extern "C" fn(Handle, *const c_void, Handle);
pub(super) type Fault = unsafe extern "C" fn(Handle, c_int, Handle);
#[cfg(test)]
type Progress = unsafe extern "C" fn(Handle, u64, u64, u32, u32, Handle);

pub(super) const OK: c_int = 0;
pub(super) const READ_EOF: c_int = 1;
pub(super) const READ_ABORT: c_int = 2;
pub(super) const IO_ERROR: c_int = 1;
pub(super) const WRITE_ABORT: c_int = 1;
pub(super) const END_OF_STREAM: c_int = 4;
pub(super) const SAMPLE_NUMBER: c_int = 1;

#[repr(C)]
pub(super) union FrameNumber {
    pub frame: u32,
    pub sample: u64,
}

// Public FLAC__FrameHeader, the first member of FLAC__Frame. Using the
// decoder's get_blocksize() here would overread a seek-trimmed block.
#[repr(C)]
pub(super) struct FrameHeader {
    pub blocksize: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_assignment: c_int,
    pub bits_per_sample: u32,
    pub number_type: c_int,
    pub number: FrameNumber,
    pub crc: u8,
}

// Checked against the pinned MSYS2 package headers with the C compiler for
// both target ABIs. These assertions also catch a local declaration drift.
const _: () = {
    assert!(size_of::<FrameHeader>() == 40);
    assert!(align_of::<FrameHeader>() == 8);
    assert!(std::mem::offset_of!(FrameHeader, number) == 24);
    assert!(std::mem::offset_of!(FrameHeader, crc) == 32);
};

type Init = unsafe extern "C" fn(
    Handle,
    Option<Read>,
    Option<Seek>,
    Option<Tell>,
    Option<Tell>,
    Option<Eof>,
    Option<Write>,
    Option<Metadata>,
    Option<Fault>,
    Handle,
) -> c_int;

pub(super) struct Api {
    _library: Library,
    pub new: unsafe extern "C" fn() -> Handle,
    pub delete: unsafe extern "C" fn(Handle),
    pub init: Init,
    pub metadata: unsafe extern "C" fn(Handle) -> c_int,
    pub process: unsafe extern "C" fn(Handle) -> c_int,
    pub seek: unsafe extern "C" fn(Handle, u64) -> c_int,
    pub total: unsafe extern "C" fn(Handle) -> u64,
    pub state: unsafe extern "C" fn(Handle) -> c_int,
    pub finish: unsafe extern "C" fn(Handle) -> c_int,
}

impl Api {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() || !path.is_file() {
            return Err(io::Error::other(
                "FLAC probe needs a trusted absolute SDK path",
            ));
        }
        #[cfg(windows)]
        let library = {
            use libloading::os::windows::{
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
                Library as WindowsLibrary,
            };
            // SAFETY: an opt-in host operator selects the trusted DLL. Its
            // dependencies are resolved from its directory and System32.
            unsafe {
                WindowsLibrary::load_with_flags(
                    path,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            }
            .map(Library::from)
        };
        #[cfg(not(windows))]
        // SAFETY: an opt-in host operator selects the trusted system library.
        let library = unsafe { Library::new(path) };
        let library = library.map_err(io::Error::other)?;
        // SAFETY: FLAC__VERSION_STRING is a const char pointer in the public
        // ABI. The trusted library owns the pointer and its NUL-terminated data.
        let version = unsafe {
            let version = library
                .get::<*const *const c_char>(b"FLAC__VERSION_STRING\0")
                .map_err(io::Error::other)?;
            CStr::from_ptr(**version)
        };
        if version.to_bytes() != b"1.5.0" {
            return Err(io::Error::other("FLAC proof is pinned to libFLAC 1.5.0"));
        }
        // SAFETY: all copied symbol types match libFLAC 1.5.0 stream_decoder.h;
        // the Api owns the library for the entire decoder lifetime.
        unsafe {
            Ok(Self {
                new: *library
                    .get(b"FLAC__stream_decoder_new\0")
                    .map_err(io::Error::other)?,
                delete: *library
                    .get(b"FLAC__stream_decoder_delete\0")
                    .map_err(io::Error::other)?,
                init: *library
                    .get(b"FLAC__stream_decoder_init_stream\0")
                    .map_err(io::Error::other)?,
                metadata: *library
                    .get(b"FLAC__stream_decoder_process_until_end_of_metadata\0")
                    .map_err(io::Error::other)?,
                process: *library
                    .get(b"FLAC__stream_decoder_process_single\0")
                    .map_err(io::Error::other)?,
                seek: *library
                    .get(b"FLAC__stream_decoder_seek_absolute\0")
                    .map_err(io::Error::other)?,
                total: *library
                    .get(b"FLAC__stream_decoder_get_total_samples\0")
                    .map_err(io::Error::other)?,
                state: *library
                    .get(b"FLAC__stream_decoder_get_state\0")
                    .map_err(io::Error::other)?,
                finish: *library
                    .get(b"FLAC__stream_decoder_finish\0")
                    .map_err(io::Error::other)?,
                _library: library,
            })
        }
    }

    /// Encode only the synthetic fixture in-process, with the same SDK.
    #[cfg(test)]
    pub(super) fn encode_fixture(
        &self,
        path: &Path,
        rate: u32,
        blocksize: u32,
        frames: u64,
        sample: impl Fn(u64) -> [i32; 2],
    ) -> io::Result<()> {
        self.encode_fixture_with_index(path, rate, blocksize, frames, None, sample)
    }

    #[cfg(test)]
    pub(super) fn encode_fixture_with_index(
        &self,
        path: &Path,
        rate: u32,
        blocksize: u32,
        frames: u64,
        seek_spacing: Option<u32>,
        sample: impl Fn(u64) -> [i32; 2],
    ) -> io::Result<()> {
        struct Object {
            handle: NonNull<c_void>,
            delete: unsafe extern "C" fn(Handle),
        }
        impl Drop for Object {
            fn drop(&mut self) {
                // SAFETY: owns the native object and no callbacks escape.
                unsafe { (self.delete)(self.handle.as_ptr()) }
            }
        }
        let file = CString::new(
            path.to_str()
                .ok_or_else(|| io::Error::other("fixture path must be UTF-8"))?,
        )?;
        // SAFETY: all symbols have the declared libFLAC 1.5.0 encoder ABI.
        // The encoder is deleted before the library reference can end; init
        // receives a valid UTF-8 filename as required on Windows and no callbacks.
        unsafe {
            let table = if let Some(spacing) = seek_spacing {
                if spacing == 0 || frames == 0 {
                    return Err(io::Error::other("invalid synthetic FLAC seektable"));
                }
                let create = self
                    ._library
                    .get::<unsafe extern "C" fn(c_int) -> Handle>(b"FLAC__metadata_object_new\0")
                    .map_err(io::Error::other)?;
                let delete = self
                    ._library
                    .get::<unsafe extern "C" fn(Handle)>(b"FLAC__metadata_object_delete\0")
                    .map_err(io::Error::other)?;
                let append = self._library.get::<unsafe extern "C" fn(Handle, u32, u64) -> c_int>(b"FLAC__metadata_object_seektable_template_append_spaced_points_by_samples\0").map_err(io::Error::other)?;
                let sort = self
                    ._library
                    .get::<unsafe extern "C" fn(Handle, c_int) -> c_int>(
                        b"FLAC__metadata_object_seektable_template_sort\0",
                    )
                    .map_err(io::Error::other)?;
                let handle = NonNull::new(create(3))
                    .ok_or_else(|| io::Error::other("cannot allocate FLAC seektable"))?;
                let table = Object {
                    handle,
                    delete: *delete,
                };
                if append(handle.as_ptr(), spacing, frames) == 0 || sort(handle.as_ptr(), 1) == 0 {
                    return Err(io::Error::other("could not initialize FLAC seektable"));
                }
                Some(table)
            } else {
                None
            };
            // Both the metadata object and its pointer array outlive the
            // encoder, which fills the offsets during finish().
            let mut metadata = table.as_ref().map(|table| [table.handle.as_ptr()]);
            let new = self
                ._library
                .get::<unsafe extern "C" fn() -> Handle>(b"FLAC__stream_encoder_new\0")
                .map_err(io::Error::other)?;
            let delete = self
                ._library
                .get::<unsafe extern "C" fn(Handle)>(b"FLAC__stream_encoder_delete\0")
                .map_err(io::Error::other)?;
            let handle = NonNull::new(new())
                .ok_or_else(|| io::Error::other("cannot allocate FLAC encoder"))?;
            let _encoder = Object {
                handle,
                delete: *delete,
            };
            let handle = handle.as_ptr();
            if let Some(metadata) = metadata.as_mut() {
                let set = self
                    ._library
                    .get::<unsafe extern "C" fn(Handle, *mut Handle, u32) -> c_int>(
                        b"FLAC__stream_encoder_set_metadata\0",
                    )
                    .map_err(io::Error::other)?;
                if set(handle, metadata.as_mut_ptr(), 1) == 0 {
                    return Err(io::Error::other("FLAC encoder seektable rejected"));
                }
            }
            for (key, value) in [
                (c"FLAC__stream_encoder_set_channels", 2),
                (c"FLAC__stream_encoder_set_bits_per_sample", 16),
                (c"FLAC__stream_encoder_set_sample_rate", rate),
                (c"FLAC__stream_encoder_set_blocksize", blocksize),
            ] {
                let setter = self
                    ._library
                    .get::<unsafe extern "C" fn(Handle, u32) -> c_int>(key.to_bytes_with_nul())
                    .map_err(io::Error::other)?;
                if setter(handle, value) == 0 {
                    return Err(io::Error::other("FLAC encoder setting rejected"));
                }
            }
            let total = self
                ._library
                .get::<unsafe extern "C" fn(Handle, u64) -> c_int>(
                    b"FLAC__stream_encoder_set_total_samples_estimate\0",
                )
                .map_err(io::Error::other)?;
            let init = self._library.get::<unsafe extern "C" fn(Handle, *const c_char, Option<Progress>, Handle) -> c_int>(b"FLAC__stream_encoder_init_file\0").map_err(io::Error::other)?;
            let process = self
                ._library
                .get::<unsafe extern "C" fn(Handle, *const i32, u32) -> c_int>(
                    b"FLAC__stream_encoder_process_interleaved\0",
                )
                .map_err(io::Error::other)?;
            let finish = self
                ._library
                .get::<unsafe extern "C" fn(Handle) -> c_int>(b"FLAC__stream_encoder_finish\0")
                .map_err(io::Error::other)?;
            if total(handle, frames) == 0
                || init(handle, file.as_ptr(), None, std::ptr::null_mut()) != OK
            {
                return Err(io::Error::other(
                    "could not initialize synthetic FLAC encoder",
                ));
            }
            let mut block = [0_i32; 2048];
            let mut cursor = 0;
            while cursor < frames {
                let count = (frames - cursor).min(1024) as usize;
                for (offset, out) in block.chunks_exact_mut(2).take(count).enumerate() {
                    out.copy_from_slice(&sample(cursor + offset as u64));
                }
                if process(handle, block.as_ptr(), count as u32) == 0 {
                    return Err(io::Error::other("synthetic FLAC encoding failed"));
                }
                cursor += count as u64;
            }
            if finish(handle) == 0 {
                return Err(io::Error::other("synthetic FLAC finish failed"));
            }
        }
        Ok(())
    }
}
