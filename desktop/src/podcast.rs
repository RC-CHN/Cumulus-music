use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn scene_fm_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<SceneFmView>, ClientError> {
    client
        .scene_fm_view(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn scene_fm_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: SceneFmCommand,
) -> Result<AccountData<SceneFmView>, ClientError> {
    client
        .scene_fm_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_state(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<PodcastState>, ClientError> {
    client
        .podcast_state(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PodcastQuery,
    refresh: bool,
) -> Result<PodcastPage, ClientError> {
    client
        .podcast_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: PodcastCommand,
) -> Result<AccountData<PodcastMutationView>, ClientError> {
    client
        .podcast_command(&epoch, command)
        .await
        .map_err(Into::into)
}
