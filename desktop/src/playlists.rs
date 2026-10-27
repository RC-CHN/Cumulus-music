use cumulus_core::{
    client::{ClientError, MusicClient, dto::*},
    playlist::PlaylistCommand,
};
#[tauri::command]
pub async fn playlist_tags(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<PlaylistTag>>, ClientError> {
    client.playlist_tags(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_editor(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<PlaylistEditor>, ClientError> {
    client
        .playlist_editor(&epoch, &id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_order_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<PlaylistOrderView>, ClientError> {
    client.playlist_order_view(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub async fn mutate_playlist(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: PlaylistCommand,
) -> Result<AccountData<PlaylistMutationView>, ClientError> {
    client
        .mutate_playlist(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn reconcile_playlist(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    operation_id: String,
) -> Result<AccountData<PlaylistMutationView>, ClientError> {
    client
        .reconcile_playlist(&epoch, &operation_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn playlist_operations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<PlaylistMutationView>>, ClientError> {
    client.playlist_operations(&epoch).map_err(Into::into)
}
