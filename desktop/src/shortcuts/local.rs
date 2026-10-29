//! Acknowledged listeners, scoped to each actual WebView. A reload or replacement invalidates its
//! lease before later IPC can act; media/account fencing remains in core.
use super::{
    dispatch,
    host::{self, Host},
};
use cumulus_core::{
    Error, Result,
    client::{ClientError, MusicClient, native_media::ShortcutSelection},
    preferences::shortcuts::{Action, Capture, Chord, Hotkey, KeyInput, Scope},
};
use serde::{Deserialize, Serialize};
use tauri::Manager;

pub(super) struct Listener {
    pub id: String,
    pub epoch: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    pub id: String,
    pub ticket: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalPress {
    pub lease: String,
    pub ticket: String,
    pub epoch: String,
    pub keys: Vec<u16>,
    pub selection: ShortcutSelection,
}
#[tauri::command]
pub fn shortcut_capture(input: KeyInput, scope: Scope) -> Capture {
    input.capture(scope == Scope::Global)
}
#[tauri::command]
pub async fn shortcut_attach<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    epoch: String,
    ticket: String,
) -> std::result::Result<Lease, ClientError> {
    let host = app
        .try_state::<Host>()
        .ok_or(Error::State("shortcut host unavailable"))?;
    if app.state::<MusicClient>().account()?.epoch != epoch {
        return Err(Error::StaleResponse.into());
    }
    host::attach(&app, &host, window.label(), epoch, ticket).map_err(Into::into)
}
#[tauri::command]
pub fn shortcut_detach<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    lease: String,
) {
    host::detach(&app, window.label(), Some(&lease));
}
pub(crate) fn closed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    host::detach(app, label, None);
}
pub(crate) fn page_started<R: tauri::Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    closed(app, label);
    if label == "main"
        && let Some(id) = host::status(app).recording
    {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = super::recording::end(&app, &id);
        });
    }
}
pub(super) async fn execute<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
    press: LocalPress,
) -> Result<()> {
    let host = app
        .try_state::<Host>()
        .ok_or(Error::State("shortcut host unavailable"))?;
    let chord = Chord::new(press.keys)?;
    let key = host::local_key(
        &host,
        label,
        &press.epoch,
        &press.lease,
        &press.ticket,
        &chord,
    )?;
    // Claim the same in-flight slot as native global presses; do not queue a
    // local command behind another operation and reinterpret it afterward.
    let _busy = dispatch::Busy::claim(&host)?;
    let client = app.state::<MusicClient>();
    if key == Hotkey::Configured(Action::DesktopLyrics) {
        let intent = crate::desktop_lyrics::shortcuts::capture(app)?;
        if intent.epoch() != press.epoch {
            return Err(Error::StaleResponse);
        }
        host::local_key(
            &host,
            label,
            &press.epoch,
            &press.lease,
            &press.ticket,
            &chord,
        )?;
        return crate::desktop_lyrics::shortcuts::toggle(app, intent).await;
    }
    if key == Hotkey::Configured(Action::MiniMode) {
        if client.account()?.epoch != press.epoch {
            return Err(Error::StaleResponse);
        }
        let intent = crate::mini::capture_mode(app)?;
        if intent.epoch() != press.epoch {
            return Err(Error::StaleResponse);
        }
        host::local_key(
            &host,
            label,
            &press.epoch,
            &press.lease,
            &press.ticket,
            &chord,
        )?;
        return crate::mini::switch_mode(app, intent).await;
    }
    let Some(invocation) = client.capture_local_shortcut(&press.epoch, &press.selection, key)?
    else {
        return Ok(());
    };
    host::local_key(
        &host,
        label,
        &press.epoch,
        &press.lease,
        &press.ticket,
        &chord,
    )?;
    let result = client.execute_native_shortcut(&invocation).await;
    dispatch::like_changed(app, &invocation);
    result
}
#[tauri::command]
pub async fn shortcut_press<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    press: LocalPress,
) -> std::result::Result<(), ClientError> {
    execute(&app, window.label(), press)
        .await
        .map_err(Into::into)
}
