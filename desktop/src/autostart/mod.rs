//! Native, current-user registration. Never run from application startup or
//! account restore, and never expose filesystem paths/registry data over IPC.
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Registration;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::Registration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disabled,
    Enabled,
    External,
    Unavailable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    enabled: bool,
    observed: Status,
}

#[derive(Serialize)]
pub struct AutostartError {
    code: &'static str,
    message: &'static str,
}
fn error(code: &'static str, message: &'static str) -> AutostartError {
    AutostartError { code, message }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn install_executable() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let temp = std::env::temp_dir();
    if !exe.is_absolute()
        || !exe.is_file()
        || (temp.is_absolute() && exe.starts_with(&temp))
        || cfg!(target_os = "linux")
            && ["/tmp", "/var/tmp", "/run"]
                .iter()
                .any(|base| exe.starts_with(base))
    {
        return Err(invalid(
            "application executable is not at a stable location",
        ));
    }
    Ok(exe)
}

#[tauri::command]
pub async fn autostart_status() -> Status {
    install_executable()
        .and_then(Registration::new)
        .and_then(|entry| entry.status())
        .unwrap_or(Status::Unavailable)
}

#[tauri::command]
pub async fn autostart_change(change: Change) -> Result<Status, AutostartError> {
    let (expected, desired) = if change.enabled {
        (Status::Disabled, Status::Enabled)
    } else {
        (Status::Enabled, Status::Disabled)
    };
    if change.observed != expected {
        return Err(error("stale", "开机启动状态已变化，请刷新后再试。"));
    }
    let registration = install_executable()
        .and_then(Registration::new)
        .map_err(|_| error("local", "无法定位可用于开机启动的客户端。"))?;
    let result = registration
        .change(expected, desired)
        .and_then(|()| registration.status())
        .map_err(|_| error("local", "未能确认开机启动更改，请刷新系统状态。"))?;
    if result != desired {
        return Err(error("stale", "系统开机启动状态已变化，请刷新后再试。"));
    }
    Ok(result)
}
