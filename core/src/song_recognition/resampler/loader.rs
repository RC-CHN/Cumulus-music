use crate::{Error, Result};
use libloading::Library;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const AVUTIL: &str = "avutil-56.dll";
#[cfg(windows)]
const SWRESAMPLE: &str = "swresample-3.dll";
#[cfg(target_os = "linux")]
const AVUTIL: &str = "libavutil.so.56";
#[cfg(target_os = "linux")]
const SWRESAMPLE: &str = "libswresample.so.3";

const UNAVAILABLE: Error = Error::State("recognition resampler library unavailable");
const BINDING: Error = Error::State("recognition resampler library binding is ambiguous");

pub(super) struct LibraryPair {
    // Unload the consumer before its provider.
    pub swresample: Library,
    pub avutil: Library,
}

impl LibraryPair {
    pub(super) unsafe fn open(directory: &Path) -> Result<Self> {
        let avutil = checked_file(directory, AVUTIL)?;
        let swresample = checked_file(directory, SWRESAMPLE)?;
        // SAFETY: the caller explicitly vouches for the two libraries and their dependencies.
        unsafe { platform::open(&avutil, &swresample) }
    }
}

fn checked_file(directory: &Path, filename: &str) -> Result<PathBuf> {
    if !directory.is_absolute() {
        return Err(Error::InvalidInput(
            "recognition resampler directory must be absolute",
        ));
    }
    let directory = fs::canonicalize(directory).map_err(|_| UNAVAILABLE)?;
    let path = fs::canonicalize(directory.join(filename)).map_err(|_| UNAVAILABLE)?;
    if path.parent() != Some(directory.as_path()) || !path.is_file() {
        return Err(UNAVAILABLE);
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use libloading::os::unix::Library as UnixLibrary;
    use std::{
        ffi::{CStr, CString, OsStr},
        os::unix::ffi::OsStrExt,
    };

    pub(super) unsafe fn open(avutil_path: &Path, swresample_path: &Path) -> Result<LibraryPair> {
        // A separate GNU loader namespace keeps a libmpv/system avutil or another
        // equal SONAME from becoming this pair's dependency.
        let (avutil, raw) = unsafe { open_in(libc::LM_ID_NEWLM, avutil_path)? };
        let mut namespace: libc::Lmid_t = 0;
        // SAFETY: raw is held by avutil; RTLD_DI_LMID expects a writable Lmid_t.
        if unsafe {
            libc::dlinfo(
                raw,
                libc::RTLD_DI_LMID,
                (&mut namespace as *mut libc::Lmid_t).cast(),
            )
        } != 0
            || namespace == libc::LM_ID_BASE
        {
            return Err(BINDING);
        }
        let (swresample, _) = unsafe { open_in(namespace, swresample_path)? };
        // SAFETY: only compare addresses; these are not called through this type.
        let expected =
            unsafe { avutil.get::<*const ()>(b"avutil_version\0") }.map_err(|_| UNAVAILABLE)?;
        let bound =
            unsafe { swresample.get::<*const ()>(b"avutil_version\0") }.map_err(|_| BINDING)?;
        let swr = unsafe { swresample.get::<*const ()>(b"swresample_version\0") }
            .map_err(|_| UNAVAILABLE)?;
        if *expected != *bound
            || !owns_symbol(*expected, avutil_path)
            || !owns_symbol(*swr, swresample_path)
        {
            return Err(BINDING);
        }
        Ok(LibraryPair { swresample, avutil })
    }

    fn owns_symbol(address: *const (), path: &Path) -> bool {
        let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
        // SAFETY: address came from a live handle; info is writable.
        if unsafe { libc::dladdr(address.cast(), info.as_mut_ptr()) } == 0 {
            return false;
        }
        // SAFETY: success initialized the result.
        let info = unsafe { info.assume_init() };
        if info.dli_fname.is_null() {
            return false;
        }
        // SAFETY: the owning loader handle keeps its NUL-terminated pathname valid.
        let loaded = unsafe { CStr::from_ptr(info.dli_fname) };
        same_file::is_same_file(Path::new(OsStr::from_bytes(loaded.to_bytes())), path)
            .unwrap_or(false)
    }

    unsafe fn open_in(
        namespace: libc::Lmid_t,
        path: &Path,
    ) -> Result<(Library, *mut libc::c_void)> {
        let filename = CString::new(path.as_os_str().as_bytes()).map_err(|_| UNAVAILABLE)?;
        // SAFETY: filename is terminated, namespace comes from the loader, and
        // open() inherited the caller's trust in constructors/destructors.
        let raw = unsafe {
            libc::dlmopen(
                namespace,
                filename.as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        if raw.is_null() {
            return Err(UNAVAILABLE);
        }
        // SAFETY: this is an owned, successful GNU loader handle.
        let library = unsafe { UnixLibrary::from_raw(raw) };
        Ok((library.into(), raw))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library as WindowsLibrary,
    };
    use std::{
        ffi::{OsString, c_void},
        mem::size_of_val,
        os::windows::ffi::OsStringExt,
        ptr,
    };

    type Module = *mut c_void;
    const FLAGS: u32 = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32EnumProcessModules(
            process: *mut c_void,
            modules: *mut Module,
            bytes: u32,
            required_bytes: *mut u32,
        ) -> i32;
        fn GetModuleFileNameW(module: Module, name: *mut u16, size: u32) -> u32;
    }

    pub(super) unsafe fn open(avutil_path: &Path, swresample_path: &Path) -> Result<LibraryPair> {
        let expected = [(AVUTIL, avutil_path), (SWRESAMPLE, swresample_path)];
        // A dependency import by basename can reuse a loaded DLL even with
        // restricted search flags. Reject an already conflicting module before
        // running our library's initialization.
        verify_modules(&expected, false)?;
        // SAFETY: full paths and restricted dependency search; open() inherits trust.
        let avutil = unsafe { WindowsLibrary::load_with_flags(avutil_path, FLAGS) }
            .map_err(|_| UNAVAILABLE)?;
        let swresample = unsafe { WindowsLibrary::load_with_flags(swresample_path, FLAGS) }
            .map_err(|_| UNAVAILABLE)?;
        // If another module raced loading with us, reject any duplicate. A DLL
        // already bound by a static import retains a loader reference here.
        verify_modules(&expected, true)?;
        Ok(LibraryPair {
            swresample: swresample.into(),
            avutil: avutil.into(),
        })
    }

    fn verify_modules(expected: &[(&str, &Path); 2], require_both: bool) -> Result<()> {
        let mut modules = [ptr::null_mut(); 1024];
        let mut bytes = 0;
        // SAFETY: current process pseudo-handle and buffers are valid for their declared sizes.
        if unsafe {
            K32EnumProcessModules(
                GetCurrentProcess(),
                modules.as_mut_ptr(),
                size_of_val(&modules) as u32,
                &mut bytes,
            )
        } == 0
            || bytes as usize > size_of_val(&modules)
            || !(bytes as usize).is_multiple_of(size_of::<Module>())
        {
            return Err(BINDING);
        }
        let mut matches = [0_u8; 2];
        let mut name = vec![0_u16; 32_768];
        for &module in &modules[..bytes as usize / size_of::<Module>()] {
            // SAFETY: module comes from the loader; the output buffer is sized.
            let written =
                unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) }
                    as usize;
            if written == 0 || written >= name.len() {
                return Err(BINDING);
            }
            let path = PathBuf::from(OsString::from_wide(&name[..written]));
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            for (i, &(basename, target)) in expected.iter().enumerate() {
                if file.eq_ignore_ascii_case(basename) {
                    matches[i] += 1;
                    if matches[i] != 1 || !same_file::is_same_file(&path, target).unwrap_or(false) {
                        return Err(BINDING);
                    }
                }
            }
        }
        if require_both && matches != [1, 1] {
            return Err(BINDING);
        }
        Ok(())
    }
}
