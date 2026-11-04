//! Native OS media surfaces. Business commands stay in the independent core.
#[cfg(target_os = "linux")]
mod linux;
mod policy;
pub mod settings;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;

/// The Windows host marks this only after its WinRT apartment is ready. An
/// installed MPRIS host never represents Windows SMTC support.
#[derive(Default)]
pub struct GuideHost(AtomicBool);
impl GuideHost {
    pub fn mark_ready(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[cfg(target_os = "windows")]
    fn mark_stopped(&self) {
        self.0.store(false, Ordering::Release);
    }
}

fn guide_available<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.try_state::<Arc<GuideHost>>()
        .is_some_and(|host| host.0.load(Ordering::Acquire))
        && matches!(
            policy::status(app).phase,
            policy::Phase::Disabled
                | policy::Phase::Idle
                | policy::Phase::Connecting
                | policy::Phase::Active
        )
}

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
