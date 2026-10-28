use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn catalog_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: CatalogQuery,
    refresh: bool,
) -> Result<AccountData<Option<CatalogView>>, ClientError> {
    client
        .catalog_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
