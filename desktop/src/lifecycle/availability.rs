//! Native tray registration is not proof that a desktop can expose its menu.
//! The monitor never calls application/business commands.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{Monitor, library, monitor};

#[cfg(target_os = "windows")]
pub struct Monitor(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(target_os = "windows")]
impl Drop for Monitor {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
#[cfg(target_os = "windows")]
pub fn monitor<R: tauri::Runtime>(
    tray: tauri::tray::TrayIcon<R>,
    changed: impl Fn(bool) + Send + Sync + 'static,
) -> Monitor {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let done = stopped.clone();
    tauri::async_runtime::spawn(async move {
        while !done.load(Ordering::Acquire) {
            // Pinned tray-icon keeps a hidden window even if NIM_ADD fails.
            // rect() asks Shell_NotifyIconGetRect about this exact item.
            let available = tray.rect().ok().flatten().is_some();
            if done.load(Ordering::Acquire) {
                break;
            }
            changed(available);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    Monitor(stopped)
}
