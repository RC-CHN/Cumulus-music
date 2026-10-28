use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn collection_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: CollectionQuery,
    refresh: bool,
) -> Result<AccountData<Option<CollectionView>>, ClientError> {
    client
        .collection_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
