use super::policy::Mode;
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_AGE: u64 = 7 * 24 * 60 * 60;

#[derive(Serialize, Deserialize)]
struct Record {
    schema: u32,
    environment: String,
    mode: Mode,
    verified_at: u64,
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn path() -> Option<PathBuf> {
    let base = std::env::var_os("CUMULUS_DATA_DIR")
        .map(|p| PathBuf::from(p).join("cache"))
        .or_else(|| dirs::cache_dir().map(|p| p.join("io.cumulus.music")));
    base.map(|p| p.join("renderer-v1.json"))
}

pub(super) fn read(path: &Path, environment: &str, now: u64) -> Option<Mode> {
    if fs::metadata(path).ok()?.len() > 4096 {
        return None;
    }
    let record: Record = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (record.schema == 1
        && record.environment == environment
        && now >= record.verified_at
        && now - record.verified_at < MAX_AGE)
        .then_some(record.mode)
}

pub(super) fn write(path: &Path, environment: &str, mode: Mode, now: u64) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("renderer cache directory"))?;
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(
        pending.as_file_mut(),
        &Record {
            schema: 1,
            environment: environment.into(),
            mode,
            verified_at: now,
        },
    )?;
    pending.flush()?;
    pending.as_file().sync_all()?;
    pending.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Local cache invalidation only. No machine/user serials and no network upload.
pub(super) fn environment(diagnostic: bool) -> String {
    #[cfg(not(target_os = "windows"))]
    let _ = diagnostic;
    let mut hash = DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hash);
    std::env::consts::OS.hash(&mut hash);
    #[cfg(target_os = "windows")]
    {
        let version = tauri::webview_version().ok();
        version.hash(&mut hash);
        if diagnostic {
            let version = version
                .as_deref()
                .filter(|value| {
                    value.len() <= 64
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'.' | b' ' | b'-' | b'_')
                        })
                })
                .unwrap_or("unavailable");
            eprintln!("renderer probe: discovered WebView2 runtime {version}");
            const OVERRIDES: &[&str] = &[
                "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
                "WEBVIEW2_USER_DATA_FOLDER",
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
                "WEBVIEW2_CHANNEL_SEARCH_KIND",
                "WEBVIEW2_RELEASE_CHANNELS",
                "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
                "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            ];
            for name in OVERRIDES {
                if std::env::var_os(name).is_some_and(|value| !value.is_empty()) {
                    eprintln!("renderer probe: inherited {name} override (value withheld)");
                }
            }
        }
    }
    if let Ok(exe) = std::env::current_exe().and_then(fs::metadata) {
        exe.len().hash(&mut hash);
        exe.modified().ok().hash(&mut hash);
    }
    for name in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "GDK_BACKEND",
        "XDG_SESSION_TYPE",
        "WEBKIT_FORCE_DMABUF_RENDERER",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "WEBKIT_FORCE_COMPOSITING_MODE",
        "__NV_DISABLE_EXPLICIT_SYNC",
        "LIBGL_ALWAYS_SOFTWARE",
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    ] {
        std::env::var_os(name).hash(&mut hash);
    }
    #[cfg(target_os = "linux")]
    {
        // Version queries do not initialize GTK or create worker threads.
        unsafe {
            (
                webkit2gtk::ffi::webkit_get_major_version(),
                webkit2gtk::ffi::webkit_get_minor_version(),
                webkit2gtk::ffi::webkit_get_micro_version(),
            )
                .hash(&mut hash);
        }
        for file in ["/proc/sys/kernel/osrelease", "/proc/driver/nvidia/version"] {
            fs::read(file).ok().hash(&mut hash);
        }
        let mut devices = Vec::new();
        for entry in fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            if entry.file_name().to_string_lossy().starts_with("renderD") {
                devices.push((
                    entry.file_name(),
                    fs::read_link(entry.path().join("device/driver")).ok(),
                    fs::read(entry.path().join("device/vendor")).ok(),
                    fs::read(entry.path().join("device/device")).ok(),
                ));
            }
        }
        devices.sort();
        devices.hash(&mut hash);
    }
    format!("{:016x}", hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_atomic_expiring_environment_scoped_and_corruption_tolerant() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache/result.json");
        assert_eq!(read(&path, "driver-a", 100), None);
        write(&path, "driver-a", Mode::GpuShm, 100).unwrap();
        assert_eq!(read(&path, "driver-a", 101), Some(Mode::GpuShm));
        assert_eq!(read(&path, "driver-b", 101), None);
        assert_eq!(read(&path, "driver-a", 99), None);
        assert_eq!(read(&path, "driver-a", 100 + MAX_AGE), None);
        write(&path, "driver-a", Mode::Software, 200).unwrap();
        assert_eq!(read(&path, "driver-a", 201), Some(Mode::Software));
        fs::write(&path, b"{broken").unwrap();
        assert_eq!(read(&path, "driver-a", 201), None);
    }
}
