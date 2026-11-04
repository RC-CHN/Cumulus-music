use cumulus_core::{
    client::{ClientError, MusicClient, dto::*},
    history::{HistorySyncAction, HistorySyncState},
};

#[tauri::command]
pub async fn history_sync_preference(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: HistorySyncAction,
) -> Result<AccountData<HistorySyncState>, ClientError> {
    client
        .history_sync_preference(&epoch, action)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn history_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: HistoryKind,
    refresh: bool,
) -> Result<AccountData<HistoryPage>, ClientError> {
    client
        .history_page(&epoch, kind, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn history_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: HistoryKind,
    command: HistoryCommand,
) -> Result<AccountData<HistoryPage>, ClientError> {
    client
        .history_command(&epoch, kind, command)
        .await
        .map_err(Into::into)
}
