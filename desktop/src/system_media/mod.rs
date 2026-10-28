//! Native OS media surfaces. Business commands stay in the independent core.
#[cfg(target_os = "linux")]
mod linux;
mod policy;
pub mod settings;
#[cfg(target_os = "windows")]
mod windows;

pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if !policy::install(app) {
        return;
    }
    #[cfg(target_os = "linux")]
    linux::install(app);
    #[cfg(target_os = "windows")]
    windows::install(app);
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let _ = app;
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "linux")]
    linux::stop(app);
    #[cfg(target_os = "windows")]
    windows::stop(app);
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let _ = app;
}
