use cumulus_core::client::{
    ClientError, MusicClient,
    dto::AccountData,
    style_tags::{StyleCategory, StyleTagOperation},
};

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

#[tauri::command]
pub fn style_tag_operations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<StyleTagOperation>>, ClientError> {
    client.style_tag_operations(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn save_style_tag(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    expected_revision: String,
    category_id: String,
    tag_id: String,
) -> Result<AccountData<StyleTagOperation>, ClientError> {
    client
        .save_style_tag(&epoch, &id, &expected_revision, &category_id, &tag_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn reconcile_style_tag(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<StyleTagOperation>, ClientError> {
    client
        .reconcile_style_tag(&epoch, &id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn accept_current_style_tag(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    observed_revision: String,
) -> Result<AccountData<StyleTagOperation>, ClientError> {
    client
        .accept_current_style_tag(&epoch, &id, &observed_revision)
        .await
        .map_err(Into::into)
}
