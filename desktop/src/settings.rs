use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn startup_preferences(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<StartupPreferencesView>, ClientError> {
    client.startup_preferences(&epoch).map_err(Into::into)
}
#[tauri::command]
pub async fn startup_preferences_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: StartupPreferencesCommand,
) -> Result<AccountData<StartupPreferencesView>, ClientError> {
    client
        .startup_preferences_command(&epoch, command)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn account_settings_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<AccountSettingsPage>, ClientError> {
    client
        .account_settings_page(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn account_settings_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: AccountSettingsCommand,
) -> Result<AccountData<AccountSettingsPage>, ClientError> {
    client
        .account_settings_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn output_settings(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<OutputView>, ClientError> {
    client
        .output_settings(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn output_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: OutputCommand,
) -> Result<AccountData<OutputView>, ClientError> {
    client.output_command(&epoch, command).map_err(Into::into)
}
