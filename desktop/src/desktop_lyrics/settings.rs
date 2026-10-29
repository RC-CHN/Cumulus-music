use super::*;
use cumulus_core::{
    client::{
        ClientError,
        dto::{DesktopLyricPreferencesCommand, DesktopLyricPreferencesView},
    },
    preferences::desktop_lyrics::Edit,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    epoch: String,
    preferences: DesktopLyricPreferencesView,
    colors: Option<cumulus_core::preferences::desktop_lyrics::Colors>,
    visible: bool,
    opening: bool,
    locked: bool,
    error: Option<&'static str>,
}
fn view<R: Runtime>(app: &tauri::AppHandle<R>, epoch: &str) -> Result<SettingsView> {
    let preferences = app
        .state::<MusicClient>()
        .desktop_lyric_preferences(epoch)?
        .value;
    let host = app.state::<Host>();
    let state = host.lock()?;
    let colors = preferences
        .current
        .as_ref()
        .map(|p| p.appearance.palette.colors());
    Ok(SettingsView {
        epoch: epoch.into(),
        preferences,
        colors,
        visible: state.window.as_ref().is_some_and(|w| w.ready),
        opening: state.window.as_ref().is_some_and(|w| !w.ready),
        locked: state.window.as_ref().is_some_and(|w| w.locked),
        error: state.error,
    })
}
pub(super) async fn apply<R: Runtime>(app: &tauri::AppHandle<R>, move_window: bool) -> Result<()> {
    let preferences = app
        .state::<MusicClient>()
        .desktop_lyric_preferences_native()?;
    let Some(value) = preferences.current.filter(|v| v.enabled) else {
        return gui(app, |app| {
            dismiss(&app, None);
            app.state::<Host>().lock()?.error = None;
            Ok(())
        })
        .await;
    };
    let exists = app.state::<Host>().lock()?.window.is_some();
    if !exists {
        return window::open(app, value).await;
    }
    gui(app, move |app| {
        let label = app
            .state::<Host>()
            .lock()?
            .window
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .label
            .clone();
        let window = app.get_webview_window(&label).ok_or(Error::StaleResponse)?;
        window
            .set_always_on_top(value.appearance.always_on_top)
            .map_err(|_| Error::State("desktop lyric pin failed"))?;
        if move_window {
            let fitted = geometry::fit(&app, &value)?;
            let (minimum_width, minimum_height) = geometry::minimum(&value.appearance);
            window
                .set_min_size(Some(tauri::LogicalSize::new(
                    minimum_width.min(f64::from(fitted.width)),
                    minimum_height.min(f64::from(fitted.height)),
                )))
                .map_err(|_| Error::State("desktop lyric minimum size failed"))?;
            window
                .set_size(tauri::LogicalSize::new(fitted.width, fitted.height))
                .map_err(|_| Error::State("desktop lyric resize failed"))?;
            window
                .set_position(tauri::LogicalPosition::new(fitted.x, fitted.y))
                .map_err(|_| Error::State("desktop lyric move failed"))?;
        }
        app.state::<Host>().lock()?.error = None;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_lyric_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
) -> std::result::Result<SettingsView, ClientError> {
    tauri::async_runtime::spawn_blocking(move || view(&app, &epoch))
        .await
        .map_err(|_| Error::State("desktop lyric settings unavailable"))?
        .map_err(Into::into)
}
#[tauri::command]
pub async fn desktop_lyric_change<R: Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    command: DesktopLyricPreferencesCommand,
) -> std::result::Result<SettingsView, ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let move_window = matches!(&command, DesktopLyricPreferencesCommand::Repair)
        || matches!(&command, DesktopLyricPreferencesCommand::Change { edit, .. } if matches!(edit.as_ref(), Edit::Appearance{..} | Edit::Geometry{..} | Edit::Reset));
    let worker = app.clone();
    let owner = epoch.clone();
    tauri::async_runtime::spawn_blocking(move || {
        worker
            .state::<MusicClient>()
            .desktop_lyric_preferences_command(&owner, command)
    })
    .await
    .map_err(|_| Error::State("desktop lyric settings unavailable"))??;
    if let Err(error) = apply(&app, move_window).await {
        host.lock()?.error = Some("设置已保存，但歌词窗口未能应用；请重试打开");
        return Err(error.into());
    }
    view(&app, &epoch).map_err(Into::into)
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowCommand {
    Retry,
    Unlock,
}
#[tauri::command]
pub async fn desktop_lyric_window_command<R: Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    action: WindowCommand,
) -> std::result::Result<SettingsView, ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    app.state::<MusicClient>()
        .desktop_lyric_preferences(&epoch)?;
    match action {
        WindowCommand::Retry => apply(&app, false).await?,
        WindowCommand::Unlock => {
            let owner = epoch.clone();
            gui(&app, move |app| {
                app.state::<MusicClient>()
                    .desktop_lyric_preferences(&owner)?;
                unlock(&app)
            })
            .await?;
        }
    }
    view(&app, &epoch).map_err(Into::into)
}
