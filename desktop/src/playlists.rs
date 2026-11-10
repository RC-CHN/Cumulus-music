use cumulus_core::{
    client::{ClientError, MusicClient, dto::*},
    playlist::{
        ExternalPlaylistInput, PlaylistCommand, PlaylistLocalChange, PlaylistMixedOrderRow,
        PlaylistMixedReorderReceipt, preview_external_playlist_inputs,
    },
};
#[tauri::command]
pub fn preview_external_playlist(
    inputs: Vec<String>,
) -> Result<Vec<ExternalPlaylistInput>, ClientError> {
    preview_external_playlist_inputs(&inputs).map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_collect_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    sort: PlaylistCollectSort,
    page: u32,
) -> Result<AccountData<PlaylistCollectPage>, ClientError> {
    client
        .playlist_collect_page(&epoch, sort, page)
        .await
        .map_err(Into::into)
}
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
#[derive(serde::Serialize)]
pub struct PlaylistLocalChanged {
    changed: bool,
}

#[tauri::command]
pub async fn mutate_playlist_local(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
    change: PlaylistLocalChange,
) -> Result<AccountData<PlaylistLocalChanged>, ClientError> {
    let result = client
        .mutate_playlist_local(&epoch, &id, &revision, change)
        .await
        .map_err(ClientError::from)?;
    Ok(AccountData {
        epoch: result.epoch,
        value: PlaylistLocalChanged {
            changed: result.value.changed,
        },
    })
}

#[tauri::command]
pub async fn reorder_playlist_mixed(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
    rows: Vec<PlaylistMixedOrderRow>,
) -> Result<AccountData<PlaylistMixedReorderReceipt>, ClientError> {
    client
        .reorder_playlist_mixed(&epoch, &id, &revision, rows)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn move_playlist_mixed(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    revision: String,
    selected: Vec<PlaylistMixedOrderRow>,
    gap: usize,
) -> Result<AccountData<PlaylistMixedReorderReceipt>, ClientError> {
    client
        .move_playlist_mixed(&epoch, &id, &revision, selected, gap)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn playlist_mixed_reorder_receipt(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<Option<PlaylistMixedReorderReceipt>>, ClientError> {
    client
        .playlist_mixed_reorder_receipt(&epoch, &id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn reconcile_playlist_mixed_reorder(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    operation_id: String,
) -> Result<AccountData<PlaylistMixedReorderReceipt>, ClientError> {
    client
        .reconcile_playlist_mixed_reorder(&epoch, &id, &operation_id)
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
