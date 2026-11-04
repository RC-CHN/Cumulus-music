//! Attribute the proof's native Windows symbols to the exact staged modules.
use super::{Api, sdk_path};
use crate::{
    Error,
    playback::engine::{MpvConfig, MpvEngine, MpvRuntime},
};
use std::{
    ffi::{OsString, c_ulong, c_void},
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    ptr,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, length: u32) -> u32;
}

fn loaded_from(address: *const c_void, expected: &Path) -> PathBuf {
    const FROM_ADDRESS: u32 = 0x04;
    const UNCHANGED_REFCOUNT: u32 = 0x02;
    let mut module = ptr::null_mut();
    let mut path = vec![0; 32_768];
    // SAFETY: the callers hold the library leases. FROM_ADDRESS treats the
    // pointer as an address, not UTF-16, and we do not own a new module ref.
    let count = unsafe {
        assert_ne!(
            GetModuleHandleExW(
                FROM_ADDRESS | UNCHANGED_REFCOUNT,
                address.cast(),
                &mut module
            ),
            0,
            "could not resolve the native symbol's owner"
        );
        GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) as usize
    };
    assert!(
        (1..path.len()).contains(&count),
        "missing native module path"
    );
    let actual = PathBuf::from(OsString::from_wide(&path[..count]));
    assert!(
        same_file::is_same_file(&actual, expected).unwrap(),
        "native symbol came from {}, expected {}",
        actual.display(),
        expected.display()
    );
    actual
}

#[test]
#[ignore = "requires the explicit audited libFLAC and libmpv Windows proof runtime"]
fn limiter_windows_cold_flac_runtime_identity() {
    let flac_path = sdk_path();
    let mpv_path = PathBuf::from(std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").unwrap());
    let relative_flac = Path::new("libFLAC.dll");
    let relative_mpv = Path::new("libmpv-2.dll");
    // The proof driver sets CWD to the very directory containing these valid
    // names. A loader that simply tries its current directory would succeed.
    assert!(relative_flac.is_file() && relative_mpv.is_file());
    assert!(Api::open(relative_flac).is_err());
    assert!(matches!(
        MpvEngine::open(MpvConfig {
            library: Some(relative_mpv.into()),
            null_output: true,
        }),
        Err(Error::InvalidInput(
            "libmpv requires an absolute Windows path"
        ))
    ));

    let absent = flac_path.with_file_name("not-a-proof-runtime.dll");
    assert!(!absent.exists());
    assert!(Api::open(&absent).is_err());

    let flac = Api::open(&flac_path).unwrap(); // validates FLAC__VERSION_STRING
    let flac_module = loaded_from(flac.new as *const c_void, &flac_path);
    let mut mpv = MpvEngine::open(MpvConfig {
        library: Some(mpv_path.clone()),
        null_output: true,
    })
    .unwrap();
    // SAFETY: the CI driver selected the audited staged absolute DLL; this
    // documented C API returns unsigned long (32 bits on Windows).
    let lease = unsafe { MpvRuntime::open(mpv_path.as_os_str()) }.unwrap();
    let symbol =
        unsafe { lease.get::<unsafe extern "C" fn() -> c_ulong>(b"mpv_client_api_version\0") }
            .unwrap();
    let mpv_module = loaded_from(*symbol as *const c_void, &mpv_path);
    eprintln!(
        "CUMULUS_WINDOWS_COLD_FLAC_RUNTIME {}",
        serde_json::json!({
            "libflacVersion": "1.5.0",
            "libflacModule": flac_module,
            "libmpvApi": mpv.api_version(),
            "libmpvModule": mpv_module,
            "libmpvVersion": mpv.proof_runtime_version().unwrap(),
            "relativeLookupRejected": true,
        })
    );
}
