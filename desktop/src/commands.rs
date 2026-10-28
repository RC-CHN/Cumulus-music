use cumulus_core::{AppInfo, Application};
use cumulus_core::{
    account::QrStatus,
    client::{ClientError, MusicClient, dto::*},
};

/// Transport adapter only: business decisions belong to cumulus-core.
#[tauri::command]
pub fn get_app_info(application: tauri::State<'_, Application>) -> AppInfo {
    application.info()
}

#[tauri::command]
pub fn account_snapshot(client: tauri::State<'_, MusicClient>) -> Result<AccountView, ClientError> {
    client.account().map_err(Into::into)
}
#[tauri::command]
pub async fn restore_account<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountView, ClientError> {
    let result = client.restore_account().await.map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub async fn begin_qr(client: tauri::State<'_, MusicClient>) -> Result<QrView, ClientError> {
    client.begin_qr().await.map_err(Into::into)
}
#[tauri::command]
pub async fn poll_qr<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<QrStatus, ClientError> {
    let result = client.poll_qr(&operation_id).await.map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub fn cancel_qr<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<(), ClientError> {
    let result = client.cancel_qr(&operation_id).map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub async fn account_playlists(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<Vec<PlaylistView>>, ClientError> {
    client.playlists().await.map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_page(
    client: tauri::State<'_, MusicClient>,
    id: String,
    page: PageRequest,
) -> Result<AccountData<TrackPage>, ClientError> {
    client.playlist_page(&id, page).await.map_err(Into::into)
}
#[tauri::command]
pub async fn search_songs(
    client: tauri::State<'_, MusicClient>,
    keyword: String,
    page: PageRequest,
) -> Result<AccountData<TrackPage>, ClientError> {
    client
        .search_songs(&keyword, page)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn daily_songs(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<TrackPage>, ClientError> {
    client.daily_songs().await.map_err(Into::into)
}
#[tauri::command]
pub async fn liked_ids(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<Vec<String>>, ClientError> {
    client.liked_ids().await.map_err(Into::into)
}
#[tauri::command]
pub async fn set_liked(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    liked: bool,
) -> Result<AccountData<LikeResult>, ClientError> {
    client
        .set_liked(&epoch, &id, liked)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn like_recovery(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    offset: usize,
) -> Result<AccountData<LikeRecoveryView>, ClientError> {
    client.like_recovery(&epoch, offset).map_err(Into::into)
}
#[tauri::command]
pub async fn recover_like(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: LikeRecoveryCommand,
) -> Result<AccountData<LikeOperationView>, ClientError> {
    client
        .recover_like(&epoch, command)
        .await
        .map_err(Into::into)
}
