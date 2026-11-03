//! The main window can only choose a published task ID, never a path or owner.
use cumulus_core::client::{ClientError, ColdOfflinePageView, ColdOfflinePlayerView, MusicClient};
use tauri::{Manager, State};

#[tauri::command]
pub async fn cold_offline_open(
    client: State<'_, MusicClient>,
) -> Result<ColdOfflinePageView, ClientError> {
    client.cold_offline_open().await.map_err(Into::into)
}
#[tauri::command]
pub async fn cold_offline_next(
    client: State<'_, MusicClient>,
) -> Result<ColdOfflinePageView, ClientError> {
    client.cold_offline_next().await.map_err(Into::into)
}
#[tauri::command]
pub async fn cold_offline_play(
    client: State<'_, MusicClient>,
    id: String,
) -> Result<ColdOfflinePlayerView, ClientError> {
    client.cold_offline_play(id).await.map_err(Into::into)
}
#[tauri::command]
pub async fn cold_offline_stop(
    client: State<'_, MusicClient>,
) -> Result<ColdOfflinePlayerView, ClientError> {
    client.cold_offline_stop().await.map_err(Into::into)
}
#[tauri::command]
pub async fn cold_offline_status(
    client: State<'_, MusicClient>,
) -> Result<ColdOfflinePlayerView, ClientError> {
    client.cold_offline_status().await.map_err(Into::into)
}
#[tauri::command]
pub async fn cold_offline_close(client: State<'_, MusicClient>) -> Result<(), ClientError> {
    client.cold_offline_close().await.map_err(Into::into)
}

pub fn close<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(client) = app.try_state::<MusicClient>() {
            let _ = client.cold_offline_close().await;
        }
    });
}
