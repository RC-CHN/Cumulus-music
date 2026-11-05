use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn search_results(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: SearchResultQuery,
    default_receipt: Option<String>,
) -> Result<AccountData<SearchResultView>, ClientError> {
    client
        .search_results_with_default(&epoch, query, default_receipt.as_deref())
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn search_reason_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: ReasonTarget,
) -> Result<AccountData<SearchReasonPage>, ClientError> {
    client
        .search_reason_page(&epoch, target)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn search_discovery(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: SearchDiscoveryQuery,
) -> Result<AccountData<SearchDiscovery>, ClientError> {
    client
        .search_discovery(&epoch, query)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn search_history(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: SearchHistoryCommand,
) -> Result<AccountData<Vec<String>>, ClientError> {
    client.search_history(&epoch, command).map_err(Into::into)
}
