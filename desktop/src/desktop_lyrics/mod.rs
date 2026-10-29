//! A transparent lyric surface over the same account, lyric service and audio engine.
pub(crate) mod commands;
mod content;
mod geometry;
pub(crate) mod settings;
pub(crate) mod shortcuts;
mod state;
mod window;
use cumulus_core::{Error, Result, client::MusicClient};

use std::sync::Mutex;
use tauri::{Manager, Runtime};

#[derive(Default)]
pub(crate) struct Host {
    ledger: Mutex<state::Ledger>,
    operation: tokio::sync::Mutex<()>,
}
impl Host {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, state::Ledger>> {
        self.ledger
            .lock()
            .map_err(|_| Error::State("desktop lyric host unavailable"))
    }
}
pub(crate) fn owns<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) -> bool {
    app.try_state::<Host>()
        .and_then(|h| {
            h.lock()
                .ok()
                .map(|l| l.window.as_ref().is_some_and(|w| w.label == label))
        })
        .unwrap_or(false)
}
async fn gui<R: Runtime, T: Send + 'static>(
    app: &tauri::AppHandle<R>,
    action: impl FnOnce(tauri::AppHandle<R>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.send(action(handle));
    })
    .map_err(|_| Error::State("desktop lyric GUI unavailable"))?;
    receive
        .await
        .map_err(|_| Error::State("desktop lyric GUI unavailable"))?
}
pub(crate) fn dismiss<R: Runtime>(app: &tauri::AppHandle<R>, generation: Option<&str>) {
    let removed = app
        .try_state::<Host>()
        .and_then(|h| h.lock().ok().and_then(|mut s| s.take(generation)));
    if let Some(removed) = removed {
        crate::shortcuts::local::closed(app, &removed.label);
        if let Some(window) = app.get_webview_window(&removed.label) {
            let _ = window.destroy();
        }
    }
}
/// Explicit startup restoration, once after MusicClient is installed. No retry loop.
pub fn start<R: Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let host = app.state::<Host>();
        let _operation = host.operation.lock().await;
        if settings::apply(&app, false).await.is_err()
            && let Ok(mut state) = host.lock()
        {
            state.error = Some("桌面歌词暂时无法打开，请在设置中重试");
        }
    });
}
pub(crate) fn unlock<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    let label = app
        .state::<Host>()
        .lock()?
        .window
        .as_ref()
        .map(|w| w.label.clone());
    if let Some(label) = label {
        app.get_webview_window(&label)
            .ok_or(Error::StaleResponse)?
            .set_ignore_cursor_events(false)
            .map_err(|_| Error::State("desktop lyric unlock failed"))?;
        if let Some(window) = app
            .state::<Host>()
            .lock()?
            .window
            .as_mut()
            .filter(|w| w.label == label)
        {
            window.locked = false;
        }
    }
    Ok(())
}
pub(crate) fn closing<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) -> bool {
    let app = window.app_handle();
    if !owns(app, window.label()) {
        return false;
    }
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let app = app.clone();
            let generation = app
                .state::<Host>()
                .lock()
                .ok()
                .and_then(|s| s.window.as_ref().map(|w| w.generation.clone()));
            if let Some(generation) = generation {
                tauri::async_runtime::spawn(async move {
                    if commands::close(&app, &generation).await.is_err()
                        && let Ok(mut s) = app.state::<Host>().lock()
                    {
                        s.error = Some("关闭桌面歌词失败，请重试");
                    }
                });
            }
        }
        tauri::WindowEvent::Destroyed => dismiss(app, None),
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => geometry::changed(app),
        _ => (),
    }
    true
}
