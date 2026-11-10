use cumulus_core::client::{ClientError, MusicClient, dto::*};
#[tauri::command]
pub async fn audiobook_skip_settings(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    context: AudiobookSkipContext,
    action: AudiobookSkipAction,
) -> Result<AccountData<AudiobookSkipView>, ClientError> {
    client
        .audiobook_skip_settings(&epoch, context, action)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_podcast_resume<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    entry_id: String,
    action: PlayerPodcastResumeAction,
) -> Result<AccountData<Option<PlayerPodcastResumeView>>, ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("podcast resume requires the main window").into(),
        );
    }
    client
        .player_podcast_resume(&epoch, &play_id, &entry_id, action)
        .await
        .map_err(Into::into)
}
fn require_scene_main<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput("scene FM requires the main window").into());
    }
    Ok(())
}

#[tauri::command]
pub async fn scene_fm_view<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<SceneFmView>, ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_view(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn scene_fm_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: SceneFmCommand,
) -> Result<AccountData<SceneFmView>, ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_command(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn scene_fm_home_enter<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<SceneFmHomeView>>, ClientError> {
    require_scene_main(&window)?;
    client.scene_fm_home_enter(&epoch).await.map_err(Into::into)
}
#[tauri::command]
pub fn scene_fm_home_leave<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    visit: String,
) -> Result<(), ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_home_leave(&epoch, &visit)
        .map_err(Into::into)
}
#[tauri::command]
pub fn scene_fm_autoplay_preference<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<bool>, ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_autoplay_preference(&epoch)
        .map_err(Into::into)
}
#[tauri::command]
pub fn set_scene_fm_autoplay_preference<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    enabled: bool,
) -> Result<AccountData<bool>, ClientError> {
    require_scene_main(&window)?;
    client
        .set_scene_fm_autoplay_preference(&epoch, enabled)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn scene_fm_palette_read<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
    voice_id: String,
    pixel_ratio: f64,
) -> Result<AccountData<SceneFmPaletteRead>, ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_palette_read(&epoch, &revision, &voice_id, pixel_ratio)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn scene_fm_palette_sample<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    width: u32,
    height: u32,
    rgba: SceneFmPalettePixels,
) -> Result<AccountData<SceneFmPaletteResult>, ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_palette_sample(&epoch, &receipt, width, height, rgba.into_vec())
        .map_err(Into::into)
}
#[tauri::command]
pub fn scene_fm_palette_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
) -> Result<(), ClientError> {
    require_scene_main(&window)?;
    client
        .scene_fm_palette_cancel(&epoch, &receipt)
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
pub fn podcast_radio_order<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PodcastRadioOrder,
) -> Result<AccountData<Option<bool>>, ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("podcast order requires the main window").into(),
        );
    }
    client
        .podcast_radio_order(&epoch, query)
        .map_err(Into::into)
}
#[tauri::command]
pub fn podcast_purchased_target<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastPurchasedTarget,
) -> Result<AccountData<PodcastPurchasedRow>, ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "purchased podcasts require the main window",
        )
        .into());
    }
    client
        .podcast_purchased_target(&epoch, &target)
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
fn require_program_menu_main<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "podcast program menu requires the main window",
        )
        .into());
    }
    Ok(())
}
#[tauri::command]
pub async fn podcast_program_comment_count<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastProgramTarget,
) -> Result<AccountData<Option<u64>>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_program_comment_count(&epoch, target, crate::network_diagnostics::default_route())
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_open_program_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastProgramTarget,
) -> Result<(), ClientError> {
    require_program_menu_main(&window)?;
    let path = client
        .podcast_program_directory_path(&epoch, target)
        .await
        .map_err(ClientError::from)?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_liked_program_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastLikedProgramTarget,
) -> Result<AccountData<PodcastRecentProgramMenu>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_liked_program_menu(&epoch, &target, crate::network_diagnostics::default_route())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn podcast_liked_program<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastLikedProgramTarget,
) -> Result<AccountData<EpisodeView>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_liked_program(&epoch, &target)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_open_liked_program_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastLikedProgramTarget,
) -> Result<(), ClientError> {
    require_program_menu_main(&window)?;
    let path = client
        .podcast_liked_program_directory_path(&epoch, &target)
        .await
        .map_err(ClientError::from)?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_recent_program_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastRecentProgramTarget,
) -> Result<AccountData<PodcastRecentProgramMenu>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_recent_program_menu(&epoch, target, crate::network_diagnostics::default_route())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_purchased_program_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastPurchasedTarget,
) -> Result<AccountData<PodcastRecentProgramMenu>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_purchased_program_menu(
            &epoch,
            &target,
            crate::network_diagnostics::default_route(),
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_open_purchased_program_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastPurchasedTarget,
) -> Result<(), ClientError> {
    require_program_menu_main(&window)?;
    let path = client
        .podcast_purchased_program_directory_path(&epoch, &target)
        .await
        .map_err(ClientError::from)?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn podcast_recent_program<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastRecentProgramTarget,
) -> Result<AccountData<EpisodeView>, ClientError> {
    require_program_menu_main(&window)?;
    client
        .podcast_recent_program(&epoch, target)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_open_recent_program_download_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: PodcastRecentProgramTarget,
) -> Result<(), ClientError> {
    require_program_menu_main(&window)?;
    let path = client
        .podcast_recent_program_directory_path(&epoch, target)
        .await
        .map_err(ClientError::from)?;
    client
        .validate_download_epoch(&epoch)
        .map_err(ClientError::from)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn podcast_subscription_recommendations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<PodcastSubscriptionRecommendations>, ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "podcast recommendations require the main window",
        )
        .into());
    }
    client
        .podcast_subscription_recommendations(&epoch)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn podcast_clear_subscribed_badge<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    id: String,
) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "podcast badge action requires the main window",
        )
        .into());
    }
    client
        .podcast_clear_subscribed_badge(&epoch, &receipt, &id)
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
