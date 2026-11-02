//! Device shortcut intent, actual OS registrations and captured core actions.
mod dispatch;
mod host;
pub mod local;
mod native;
pub mod recording;
pub mod settings;
pub(crate) use host::stop;
pub use host::{Phase, Row, Status, install, status};

/// Read-only native dispatch state, distinct from OS registration readiness.
/// An updated song title does not mean an asynchronous shortcut has finished.
pub fn is_processing<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    app.try_state::<host::Host>()
        .is_some_and(|host| host.busy.load(std::sync::atomic::Ordering::Acquire))
}

pub(crate) fn lifecycle<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri::Manager;
    tauri::plugin::Builder::new("shortcut-lifecycle")
        .on_page_load(|view, payload| {
            if (view.label() == "main" || crate::mini::owns(view.app_handle(), view.label()))
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
            {
                local::page_started(view.app_handle(), view.label());
            }
        })
        .build()
}
