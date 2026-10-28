use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn relay_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<RelayView>, ClientError> {
    client.relay_view(&epoch, refresh).await.map_err(Into::into)
}
#[tauri::command]
pub async fn relay_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: RelayCommand,
) -> Result<AccountData<RelayView>, ClientError> {
    client
        .relay_command(&epoch, command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn relay_setting_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: RelaySettingCommand,
) -> Result<AccountData<RelayView>, ClientError> {
    client
        .relay_setting_command(&epoch, command)
        .await
        .map_err(Into::into)
}
