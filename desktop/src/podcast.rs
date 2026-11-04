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
pub async fn podcast_search_membership(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<PodcastSearchMembership>, ClientError> {
    client
        .podcast_search_membership(&epoch)
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
pub async fn podcast_recent(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PodcastRecentQuery,
) -> Result<AccountData<PodcastRecentPage>, ClientError> {
    client
        .podcast_recent(&epoch, query)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_catalogue(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PodcastCatalogueQuery,
) -> Result<AccountData<PodcastCatalogue>, ClientError> {
    client
        .podcast_catalogue(&epoch, query)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_home(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    continuation: Option<String>,
) -> Result<AccountData<PodcastHomePage>, ClientError> {
    client
        .podcast_home(&epoch, continuation.as_deref())
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_rankings(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PodcastRankQuery,
) -> Result<AccountData<PodcastRanks>, ClientError> {
    client
        .podcast_rankings(&epoch, query)
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

#[tauri::command]
pub async fn profile_podcasts(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
    query: ProfilePodcastQuery,
) -> Result<AccountData<Option<ProfilePodcastPage>>, ClientError> {
    client
        .profile_podcasts(&epoch, &uid, query)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn open_profile_podcast_management<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    uid: String,
) -> Result<(), ClientError> {
    use tauri::Manager;
    let client = app.state::<MusicClient>();
    let handoff = client
        .prepare_profile_podcast_management(&epoch, &uid)
        .await?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<MusicClient>()
            .launch_profile_podcast_management(handoff, crate::browser::launch)
    })
    .await
    .map_err(|_| ClientError::from(cumulus_core::Error::State("browser worker stopped")))?
    .map_err(Into::into)
}
