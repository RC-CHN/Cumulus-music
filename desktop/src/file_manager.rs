//! Native directory launcher shared by download and cloud resource menus.
use cumulus_core::Error;
use std::path::PathBuf;

mod reveal;
pub(crate) use reveal::reveal_directory;

pub async fn open_directory(path: PathBuf) -> cumulus_core::Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        if !path.is_absolute() || !path.is_dir() {
            return Err(Error::State("file manager directory missing"));
        }
        launch(path)
    })
    .await
    .map_err(|_| Error::State("file manager worker stopped"))?
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn launch(path: PathBuf) -> cumulus_core::Result<()> {
    use std::{process::Stdio, time::Duration};
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("/usr/bin/open");
        c.arg("--");
        c
    };
    // A verified absolute directory is one argv item, never a shell command or URL.
    let mut child = command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Some file managers keep the launcher alive for the lifetime of their window.
    // Report immediate errors, then reap without freezing IPC or killing a window
    // the user may still be using. Acceptance is not visual proof.
    for _ in 0..25 {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(Error::State("file manager failed"))
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::Builder::new()
        .name("file-manager-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(windows)]
fn launch(path: PathBuf) -> cumulus_core::Result<()> {
    // Shell extensions may require an STA; do not change Tauri's runtime threads.
    std::thread::Builder::new()
        .name("file-manager-shell".into())
        .spawn(move || windows_open(path))?
        .join()
        .map_err(|_| Error::State("file manager shell stopped"))?
}

#[cfg(windows)]
fn windows_shell_path(path: PathBuf) -> PathBuf {
    // Shell extensions need not accept canonical verbatim prefixes. Keep UNC
    // paths absolute when removing that prefix.
    match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::VerbatimDisk(drive) => {
                let rest: PathBuf = path.components().skip(2).collect();
                PathBuf::from(format!("{}:\\", drive as char)).join(rest)
            }
            std::path::Prefix::VerbatimUNC(server, share) => {
                let mut result = PathBuf::from("\\\\");
                result.push(server);
                result.push(share);
                result.extend(path.components().skip(2));
                result
            }
            _ => path,
        },
        _ => path,
    }
}

#[cfg(windows)]
fn windows_open(path: PathBuf) -> cumulus_core::Result<()> {
    windows_open_target(windows_shell_path(path).as_os_str())
}

#[cfg(windows)]
pub(crate) fn windows_open_target(target: &std::ffi::OsStr) -> cumulus_core::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let mut wide: Vec<u16> = target.encode_wide().collect();
    if wide.contains(&0) {
        return Err(Error::State("file manager directory invalid"));
    }
    wide.push(0);
    // SAFETY: this dedicated thread owns live NUL-terminated UTF-16 buffers.
    // The caller supplies a verified directory or a fixed public browser target;
    // fixed verb, no parameters/elevation.
    unsafe {
        let initialized = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        if initialized < 0 {
            return Err(Error::State("file manager COM initialization failed"));
        }
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            windows_sys::w!("open"),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        ) as isize;
        CoUninitialize();
        if result <= 32 {
            return Err(Error::State("file manager failed"));
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn launch(_: PathBuf) -> cumulus_core::Result<()> {
    Err(Error::State("file manager not supported on this platform"))
}
