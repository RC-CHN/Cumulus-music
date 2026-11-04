use cumulus_core::client::{
    ClientError, ExternalImportAuthAction, ExternalImportAuthView, MusicClient, dto::*,
};

#[tauri::command]
pub async fn external_playlist_import(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: ExternalImportAction,
) -> Result<AccountData<ExternalImportView>, ClientError> {
    client
        .external_import_command(&epoch, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn external_playlist_import_auth(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<ExternalImportAuthView, ClientError> {
    client.external_import_auth_view(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn external_playlist_import_auth_action(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: ExternalImportAuthAction,
) -> Result<ExternalImportAuthView, ClientError> {
    client
        .external_import_auth_command(&epoch, action)
        .await
        .map_err(Into::into)
}
