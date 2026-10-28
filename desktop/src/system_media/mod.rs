//! Native OS media surfaces. Business commands stay in the independent core.
#[cfg(target_os = "linux")]
mod linux;

pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "linux")]
    linux::install(app);
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "linux")]
    linux::stop(app);
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}
