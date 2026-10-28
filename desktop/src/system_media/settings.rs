use super::policy;
use cumulus_core::{
    Error, Result,
    client::{
        ClientError, MusicClient,
        dto::{AccountData, SystemMediaPreferencesCommand, SystemMediaPreferencesView},
    },
};
use tauri::Manager;
pub fn read<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    epoch: &str,
) -> Result<AccountData<SystemMediaPreferencesView>> {
    let client = app.state::<MusicClient>();
    let Some(host) = app.try_state::<policy::Host>() else {
        return client.system_media_preferences(epoch);
    };
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("system media policy unavailable"))?;
    let result = client.system_media_preferences(epoch);
    match &result {
        Ok(value) => policy::apply(app, &value.value),
        Err(Error::StaleResponse) => (),
        Err(_) => policy::unavailable(app),
    }
    result
}
pub fn change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    epoch: &str,
    command: SystemMediaPreferencesCommand,
) -> Result<AccountData<SystemMediaPreferencesView>> {
    let client = app.state::<MusicClient>();
    let Some(host) = app.try_state::<policy::Host>() else {
        return client.system_media_preferences_command(epoch, command);
    };
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("system media policy unavailable"))?;
    let value = client.system_media_preferences_command(epoch, command)?;
    policy::apply(app, &value.value);
    Ok(value)
}
#[tauri::command]
pub async fn system_media_preferences<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
) -> std::result::Result<AccountData<SystemMediaPreferencesView>, ClientError> {
    tauri::async_runtime::spawn_blocking(move || read(&app, &epoch))
        .await
        .map_err(|_| ClientError::from(Error::State("system media worker unavailable")))?
        .map_err(Into::into)
}
#[tauri::command]
pub async fn system_media_preferences_command<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    command: SystemMediaPreferencesCommand,
) -> std::result::Result<AccountData<SystemMediaPreferencesView>, ClientError> {
    tauri::async_runtime::spawn_blocking(move || change(&app, &epoch, command))
        .await
        .map_err(|_| ClientError::from(Error::State("system media worker unavailable")))?
        .map_err(Into::into)
}
#[tauri::command]
pub fn system_media_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> policy::Status {
    policy::status(&app)
}
