pub(crate) mod sync_observer;

use cumulus_core::{
    client::{ClientError, MusicClient, dto::*},
    history::{HistorySyncAction, HistorySyncState},
};

#[tauri::command]
pub async fn history_sync_preference<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: HistorySyncAction,
) -> Result<AccountData<HistorySyncState>, ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "history sync settings are main-window only",
        )
        .into());
    }
    sync_observer::observe(window.app_handle(), &client)?;
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

#[tauri::command]
pub async fn listened_books(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: ListenedBooksQuery,
) -> Result<AccountData<ListenedBooksPage>, ClientError> {
    client
        .listened_books(&epoch, query)
        .await
        .map_err(Into::into)
}
