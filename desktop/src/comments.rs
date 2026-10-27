use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn comments_floor(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
    query: CommentFloorQuery,
    refresh: bool,
) -> Result<AccountData<CommentFloorView>, ClientError> {
    client
        .comments_floor(&epoch, resource, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn comments_prepare_write(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
    command: CommentWriteCommand,
) -> Result<AccountData<CommentWriteView>, ClientError> {
    client
        .comments_prepare_write(&epoch, resource, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn comments_write_operations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
) -> Result<AccountData<Vec<CommentWriteView>>, ClientError> {
    client
        .comments_write_operations(&epoch, resource)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn comments_write_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    command: CommentWriteAction,
) -> Result<AccountData<CommentWriteView>, ClientError> {
    client
        .comments_write_command(&epoch, &id, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn comments_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
    query: CommentQuery,
    refresh: bool,
) -> Result<AccountData<CommentPageView>, ClientError> {
    client
        .comments_page(&epoch, resource, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn comments_like(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
    id: String,
    desired: bool,
) -> Result<AccountData<CommentOperationView>, ClientError> {
    client
        .comments_like(&epoch, resource, &id, desired)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn comments_reconcile(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CommentResource,
    id: String,
) -> Result<AccountData<CommentOperationView>, ClientError> {
    client
        .comments_reconcile(&epoch, resource, &id)
        .await
        .map_err(Into::into)
}
