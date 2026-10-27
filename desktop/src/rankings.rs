use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn ranking_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: RankingQuery,
    refresh: bool,
) -> Result<AccountData<RankingView>, ClientError> {
    client
        .ranking_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
