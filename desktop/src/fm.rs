use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn fm_discovery(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: FmDiscoveryQuery,
    refresh: bool,
) -> Result<AccountData<SavedFmDiscovery>, ClientError> {
    client
        .fm_discovery(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn fm_reasons(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    source: FmSource,
    entry_id: String,
) -> Result<AccountData<FmReasons>, ClientError> {
    client
        .fm_reasons(&epoch, source, &entry_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn fm_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    source: FmSource,
    refresh: bool,
) -> Result<AccountData<FmView>, ClientError> {
    client
        .fm_view(&epoch, source, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn fm_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    source: FmSource,
    command: FmCommand,
) -> Result<AccountData<FmView>, ClientError> {
    client
        .fm_command(&epoch, source, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn fm_trash(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    page: usize,
    cursor: serde_json::Value,
) -> Result<AccountData<FmTrashView>, ClientError> {
    client
        .fm_trash(&epoch, page, cursor)
        .await
        .map_err(Into::into)
}
