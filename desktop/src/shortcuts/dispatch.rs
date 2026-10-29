use super::host::{self, Host};
use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, native_media::NativeShortcutInvocation},
    preferences::shortcuts::{Action, Hotkey, Ticket},
};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};
pub(super) fn supported(key: Hotkey) -> bool {
    matches!(
        key,
        Hotkey::Configured(Action::MiniMode | Action::DesktopLyrics)
    ) || NativeShortcutInvocation::supported(key)
}
enum Invocation {
    Media(NativeShortcutInvocation),
    Window(crate::mini::ModeIntent),
    Lyrics(crate::desktop_lyrics::shortcuts::Intent),
}
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
    let captured = if key == Hotkey::Configured(Action::MiniMode) {
        crate::mini::capture_mode(app).map(|v| Some(Invocation::Window(v)))
    } else if key == Hotkey::Configured(Action::DesktopLyrics) {
        crate::desktop_lyrics::shortcuts::capture(app).map(|v| Some(Invocation::Lyrics(v)))
    } else {
        client
            .capture_native_shortcut(key)
            .map(|v| v.map(Invocation::Media))
    };
    let invocation = match captured {
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
            let result = match invocation {
                Invocation::Media(invocation) => {
                    let result = app
                        .state::<MusicClient>()
                        .execute_native_shortcut(&invocation)
                        .await;
                    like_changed(&app, &invocation);
                    result
                }
                Invocation::Window(intent) => crate::mini::switch_mode(&app, intent).await,
                Invocation::Lyrics(intent) => {
                    crate::desktop_lyrics::shortcuts::toggle(&app, intent).await
                }
            };
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
        let payload = serde_json::json!({"epoch":epoch,"error":error});
        let _ = app.emit_to("main", "cumulus:shortcut-error", &payload);
        for label in app.webview_windows().keys().filter(|label| {
            crate::mini::owns(app, label) || crate::desktop_lyrics::owns(app, label)
        }) {
            let _ = app.emit_to(label, "cumulus:shortcut-error", &payload);
        }
    }
}
