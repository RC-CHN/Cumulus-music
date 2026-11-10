//! Compiled only into the explicitly built Windows packaged diagnostic.
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub(crate) fn requested(explicit_file_open: bool) -> bool {
    if !explicit_file_open
        || std::env::var_os("CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_OUTPUT").as_deref()
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
    let disposable = state.is_absolute()
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
        });
    if disposable {
        eprintln!("Cumulus native-open proof audio: null");
    }
    disposable
}
