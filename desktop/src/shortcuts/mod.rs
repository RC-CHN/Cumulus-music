//! Device shortcut intent, actual OS registrations and captured core actions.
mod dispatch;
mod host;
pub mod local;
mod native;
pub mod recording;
pub mod settings;
pub(crate) use host::stop;
pub use host::{Phase, Row, Status, install, status};

pub(crate) fn lifecycle<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri::Manager;
    tauri::plugin::Builder::new("shortcut-lifecycle")
        .on_page_load(|view, payload| {
            if view.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
            {
                local::page_started(view.app_handle());
            }
        })
        .build()
}
