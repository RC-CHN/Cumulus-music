use cumulus_core::client::{ClientError, MusicClient, dto::*};
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
