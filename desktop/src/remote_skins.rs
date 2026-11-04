use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn remote_skin_catalog(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<RemoteSkinCatalog>, ClientError> {
    client.remote_skin_catalog(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn remote_skin_image(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    key: u16,
    kind: RemoteSkinImage,
) -> Result<AccountData<String>, ClientError> {
    client
        .remote_skin_image(&epoch, &revision, key, kind)
        .await
        .map_err(Into::into)
}
