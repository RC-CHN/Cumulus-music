//! Explicit proof-only Windows SDK selection; no frontend or product setting.
use super::ensure;
use cumulus_core::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) fn selected_mpv() -> Result<Option<PathBuf>> {
    let selected = std::env::var_os("CUMULUS_STEREO_PCM_MPV_LIBRARY").map(PathBuf::from);
    if let Some(path) = &selected {
        ensure(
            path.is_absolute() && path.is_file(),
            "proof libmpv must be an explicit trusted absolute file",
        )?;
    }
    #[cfg(windows)]
    ensure(
        selected.is_some(),
        "Windows stereo PCM proof requires the explicit staged libmpv",
    )?;
    Ok(selected)
}

pub(super) fn loaded_mpv(path: Option<&Path>) -> Result<Value> {
    #[cfg(windows)]
    {
        use cumulus_core::playback::engine::MpvRuntime;
        use std::{ffi::c_void, os::windows::ffi::OsStringExt};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void)
            -> i32;
            fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, length: u32) -> u32;
        }
        let expected = path.ok_or(cumulus_core::Error::State("missing proof libmpv"))?;
        // SAFETY: only the native test CLI supplies this trusted absolute SDK.
        let lease = unsafe { MpvRuntime::open(expected.as_os_str()) }?;
        // SAFETY: the documented mpv C ABI returns C unsigned long.
        let api = unsafe {
            lease.get::<unsafe extern "C" fn() -> std::ffi::c_ulong>(b"mpv_client_api_version\0")
        }?;
        let mut module = std::ptr::null_mut();
        // SAFETY: FROM_ADDRESS treats this borrowed live symbol as an address;
        // UNCHANGED_REFCOUNT does not transfer ownership of the module handle.
        ensure(
            unsafe { GetModuleHandleExW(4 | 2, *api as *const c_void as *const u16, &mut module) }
                != 0,
            "could not identify the actually loaded proof libmpv",
        )?;
        let mut text = vec![0u16; 32768];
        // SAFETY: the live module and writable UTF16 capacity are valid.
        let n =
            unsafe { GetModuleFileNameW(module, text.as_mut_ptr(), text.len() as u32) } as usize;
        ensure(n > 0 && n < text.len(), "proof module path truncated")?;
        let actual = PathBuf::from(std::ffi::OsString::from_wide(&text[..n]));
        ensure(
            actual.canonicalize()? == expected.canonicalize()?,
            "loaded proof libmpv differs from the staged SDK",
        )?;
        // SAFETY: the live typed symbol has no arguments and no borrowed data.
        let version = unsafe { api() };
        ensure(version > 0, "proof libmpv ABI version missing")?;
        Ok(json!({"kind":"explicit_windows_runtime","libmpvModule":actual,"libmpvApi":version}))
    }
    #[cfg(not(windows))]
    Ok(match path {
        Some(path) => json!({"kind":"explicit_native_path","requestedLibrary":path}),
        None => json!({"kind":"system_libmpv"}),
    })
}
