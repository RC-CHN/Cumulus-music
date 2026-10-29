use super::host;
use cumulus_core::{
    Error, Result,
    client::{
        ClientError, MusicClient,
        dto::{AccountData, ShortcutSettingsCommand, ShortcutSettingsView},
    },
};
use tauri::Manager;
pub fn read<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    epoch: &str,
) -> Result<AccountData<ShortcutSettingsView>> {
    let client = app.state::<MusicClient>();
    let Some(host) = app.try_state::<host::Host>() else {
        return client.shortcut_settings(epoch);
    };
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    let result = client.shortcut_settings(epoch);
    match &result {
        Ok(value) => host::apply(app, &value.value),
        Err(Error::StaleResponse) => (),
        Err(_) => host::unavailable(app),
    }
    result
}
pub fn change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    epoch: &str,
    command: ShortcutSettingsCommand,
) -> Result<AccountData<ShortcutSettingsView>> {
    let client = app.state::<MusicClient>();
    let Some(host) = app.try_state::<host::Host>() else {
        return client.shortcut_settings_command(epoch, command);
    };
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    let result = client.shortcut_settings_command(epoch, command)?;
    host::apply(app, &result.value);
    Ok(result)
}
#[tauri::command]
pub async fn shortcut_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
) -> std::result::Result<AccountData<ShortcutSettingsView>, ClientError> {
    tauri::async_runtime::spawn_blocking(move || read(&app, &epoch))
        .await
        .map_err(|_| ClientError::from(Error::State("shortcut worker unavailable")))?
        .map_err(Into::into)
}
#[tauri::command]
pub async fn shortcut_settings_command<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    command: ShortcutSettingsCommand,
) -> std::result::Result<AccountData<ShortcutSettingsView>, ClientError> {
    tauri::async_runtime::spawn_blocking(move || change(&app, &epoch, command))
        .await
        .map_err(|_| ClientError::from(Error::State("shortcut worker unavailable")))?
        .map_err(Into::into)
}
#[tauri::command]
pub fn shortcut_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> host::Status {
    host::status(&app)
}
