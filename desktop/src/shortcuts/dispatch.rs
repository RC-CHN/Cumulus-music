use super::host::{self, Host};
use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, native_media::NativeShortcutInvocation},
    preferences::shortcuts::{Hotkey, Ticket},
};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};
pub(super) struct Busy<'a>(&'a Host);
impl<'a> Busy<'a> {
    pub fn claim(host: &'a Host) -> cumulus_core::Result<Self> {
        host.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::MutationBusy)?;
        Ok(Self(host))
    }
}
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
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
    let epoch = client.account().ok().map(|a| a.epoch);
    // Capture immediately on the native event, before queuing an async task.
    let invocation = match client.capture_native_shortcut(key) {
        Ok(Some(value)) => value,
        result => {
            host.busy.store(false, Ordering::Release);
            if let Err(error) = result {
                notice(app, epoch.as_deref(), error);
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
            like_changed(&app, &invocation);
            if let Err(error) = result {
                notice(&app, epoch.as_deref(), error);
            }
        }
        host.busy.store(false, Ordering::Release);
    });
}
pub(super) fn like_changed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    invocation: &NativeShortcutInvocation,
) {
    if let Some(selection) = invocation.like_selection() {
        let _ = app.emit_to(
            "main",
            "cumulus:native-like-changed",
            serde_json::json!({"epoch":selection.epoch(),"resource":selection.resource}),
        );
    }
}
fn notice<R: tauri::Runtime>(app: &tauri::AppHandle<R>, epoch: Option<&str>, error: Error) {
    if matches!(error, Error::StaleResponse | Error::Cancelled) {
        return;
    }
    let error = ClientError::from(error);
    if let Some(epoch) = epoch {
        let _ = app.emit_to(
            "main",
            "cumulus:shortcut-error",
            serde_json::json!({"epoch":epoch,"error":error}),
        );
    }
}
