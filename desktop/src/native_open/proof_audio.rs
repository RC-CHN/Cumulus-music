//! Compiled only into the explicitly built Windows packaged diagnostic.
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

pub(crate) fn requested(explicit_file_open: bool) -> bool {
    let enabled = explicit_file_open && disposable_profile();
    if enabled {
        eprintln!("Cumulus native-open proof audio: null");
    }
    enabled
}

fn disposable_profile() -> bool {
    if std::env::var_os("CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_OUTPUT").as_deref()
        != Some(OsStr::new("null"))
    {
        return false;
    }
    let Some(state) = std::env::var_os("CUMULUS_DATA_DIR").map(PathBuf::from) else {
        return false;
    };
    let Some(profile) = state.parent() else {
        return false;
    };
    state.is_absolute()
        && state.file_name() == Some(OsStr::new("state"))
        && profile.file_name() == Some(OsStr::new("profile"))
        && profile
            .parent()
            .and_then(|directory| directory.file_name())
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("native-open-"))
        && [
            ("APPDATA", "roaming"),
            ("LOCALAPPDATA", "local"),
            ("WEBVIEW2_USER_DATA_FOLDER", "webview"),
        ]
        .into_iter()
        .all(|(name, leaf)| {
            std::env::var_os(name).is_some_and(|path| Path::new(&path) == profile.join(leaf))
        })
}

/// Fixed stage names and optional Win32 errors only. Never print paths,
/// endpoint names, tokens, process IDs, account data, or any frame bytes.
pub(crate) fn handoff_trace(stage: &'static str, os_error: Option<i32>) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static REMAINING: AtomicU8 = AtomicU8::new(96);
    if *ENABLED.get_or_init(|| {
        disposable_profile()
            && std::env::var_os("CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_TRACE").as_deref()
                == Some(OsStr::new("1"))
    }) && REMAINING
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_sub(1)
        })
        .is_ok()
    {
        if let Some(code) = os_error {
            eprintln!("Cumulus native-open proof stage: {stage} win32={code}");
        } else {
            eprintln!("Cumulus native-open proof stage: {stage}");
        }
    }
}
