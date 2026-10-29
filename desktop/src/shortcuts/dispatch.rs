use super::host::{self, Host};
use cumulus_core::{
    Error,
    client::{ClientError, MusicClient},
    preferences::shortcuts::{Hotkey, Ticket},
};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};
pub(super) fn pressed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, ticket: Ticket, key: Hotkey) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    if !host::active(&host, &ticket, key)
        || host
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    let client = app.state::<MusicClient>();
    // Capture immediately on the native event, before queuing an async task.
    let invocation = match client.capture_native_shortcut(key) {
        Ok(Some(value)) => value,
        result => {
            host.busy.store(false, Ordering::Release);
            if let Err(error) = result {
                notice(app, error);
            }
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let host = app.state::<Host>();
        if host::active(&host, &ticket, key) {
            let result = app
                .state::<MusicClient>()
                .execute_native_shortcut(&invocation)
                .await;
            if let Some(selection) = invocation.like_selection() {
                // Lost write replies require readback; never dispatch another write.
                let _ = app.emit_to(
                    "main",
                    "cumulus:native-like-changed",
                    serde_json::json!({"epoch":selection.epoch(),"resource":selection.resource}),
                );
            }
            if let Err(error) = result {
                notice(&app, error);
            }
        }
        host.busy.store(false, Ordering::Release);
    });
}
fn notice<R: tauri::Runtime>(app: &tauri::AppHandle<R>, error: Error) {
    if matches!(error, Error::StaleResponse | Error::Cancelled) {
        return;
    }
    let error = ClientError::from(error);
    let _ = app.emit_to("main", "cumulus:shortcut-error", &error);
}
