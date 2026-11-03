use cumulus_core::client::{ClientError, MusicClient, dto::AccountData, style_tags::StyleCategory};

#[tauri::command]
pub async fn style_recommendation_categories(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<StyleCategory>>, ClientError> {
    client
        .style_recommendation_categories(&epoch)
        .await
        .map_err(Into::into)
}
