pub(crate) mod sync_observer;

use cumulus_core::{
    client::{ClientError, MusicClient, dto::*},
    history::{HistorySyncAction, HistorySyncState},
};
use tauri::Manager;

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
pub async fn history_row_menu_info<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: HistoryRowMenuTarget,
) -> Result<AccountData<HistoryRowMenuInfo>, ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("history row menu is main-window only").into(),
        );
    }
    client
        .history_row_menu_info(&epoch, target)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn history_row_voice_reveal<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: HistoryRowMenuTarget,
    receipt: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "history program reveal is main-window only",
        )
        .into());
    }
    let reveal = client
        .history_row_voice_file_reveal(&epoch, target, &receipt)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        reveal.check_current(&app.state::<MusicClient>())
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub async fn history_row_song_reveal<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: HistoryRowMenuTarget,
    receipt: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("history song reveal is main-window only").into(),
        );
    }
    let reveal = client
        .history_row_song_file_reveal(&epoch, target, &receipt)
        .await?;
    let app = window.app_handle().clone();
    crate::file_manager::reveal_file_checked(reveal.path().to_owned(), move || {
        reveal.check_current(&app.state::<MusicClient>())
    })
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
pub async fn history_radio_collection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: HistoryKind,
    revision: String,
    id: String,
    action: HistoryRadioCollection,
) -> Result<AccountData<PodcastMutationView>, ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("history collection is main-window only").into(),
        );
    }
    client
        .history_radio_collection(&epoch, kind, &revision, &id, action)
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
