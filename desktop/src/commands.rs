use cumulus_core::{AppInfo, Application};
use cumulus_core::{
    account::QrStatus,
    client::{ClientError, MusicClient, dto::*},
};
use tauri::Emitter;

/// Transport adapter only: business decisions belong to cumulus-core.
#[tauri::command]
pub fn get_app_info(application: tauri::State<'_, Application>) -> AppInfo {
    application.info()
}

#[tauri::command]
pub fn account_snapshot(client: tauri::State<'_, MusicClient>) -> Result<AccountView, ClientError> {
    client.account().map_err(Into::into)
}
#[tauri::command]
pub async fn account_profile_detail(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<cumulus_core::account::profile::SelfProfileDetailView>, ClientError> {
    client
        .account_profile_detail(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn account_binding_qr(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<AccountBindingQrView>, ClientError> {
    client.account_binding_qr(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn account_profile_regions(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<cumulus_core::account::profile::regions::RegionTable>>, ClientError>
{
    client
        .account_profile_regions(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn account_profile_edit_page<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<cumulus_core::account::profile::edit::ProfileEditPage>, ClientError> {
    let result = client
        .account_profile_edit_page(&epoch, refresh)
        .await
        .map_err(Into::into);
    publish_profile(&window, &client, &epoch);
    result
}

#[tauri::command]
pub async fn account_profile_edit_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: AccountProfileEditCommand,
) -> Result<AccountData<cumulus_core::account::profile::edit::ProfileEditPage>, ClientError> {
    let result = client
        .account_profile_edit_command(&epoch, command)
        .await
        .map_err(Into::into);
    publish_profile(&window, &client, &epoch);
    result
}

fn publish_profile<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    client: &MusicClient,
    epoch: &str,
) {
    if let Ok(account) = client.account()
        && account.epoch == epoch
    {
        let _ = window.emit("cumulus:account-changed", account);
    }
}

#[tauri::command]
pub async fn account_login_devices(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<LoginDevicesView>, ClientError> {
    client.login_devices(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub fn account_login_device_removal(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<LoginDeviceRemovalView>>, ClientError> {
    client.login_device_removal(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn account_login_device_removal_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: LoginDeviceRemovalCommand,
) -> Result<AccountData<LoginDeviceRemovalView>, ClientError> {
    let result = client
        .login_device_removal_command(&epoch, command)
        .await
        .map_err(Into::into);
    // A fresh authenticated read can invalidate the session during the flow.
    if client.account().is_ok_and(|account| account.epoch != epoch) {
        crate::auth::publish(&window, &client);
    }
    result
}

#[tauri::command]
pub async fn restore_account<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountView, ClientError> {
    let result = client.restore_account().await.map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub async fn begin_qr(client: tauri::State<'_, MusicClient>) -> Result<QrView, ClientError> {
    client.begin_qr().await.map_err(Into::into)
}
#[tauri::command]
pub async fn poll_qr<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<QrStatus, ClientError> {
    let result = client.poll_qr(&operation_id).await.map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub fn qr_identity_pending(
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<bool, ClientError> {
    client
        .qr_identity_pending(&operation_id)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn reconcile_qr<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<(), ClientError> {
    let result = client.reconcile_qr(&operation_id).await.map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub fn cancel_qr<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    operation_id: String,
) -> Result<(), ClientError> {
    let result = client.cancel_qr(&operation_id).map_err(Into::into);
    crate::auth::publish(&window, &client);
    result
}
#[tauri::command]
pub async fn account_playlists(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<Vec<PlaylistView>>, ClientError> {
    client.playlists().await.map_err(Into::into)
}
#[tauri::command]
pub async fn playlist_creator_profile(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    uid: String,
) -> Result<AccountData<Option<PlaylistCreatorProfile>>, ClientError> {
    client
        .playlist_creator_profile(&epoch, &uid)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_essential_categories(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<PlaylistEssentialCategory>>, ClientError> {
    client
        .playlist_essential_categories(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_essential_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    category: String,
    continuation: Option<String>,
) -> Result<AccountData<PlaylistEssentialPage>, ClientError> {
    client
        .playlist_essential_page(&epoch, &category, continuation.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_square_recommendations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    continuation: Option<String>,
) -> Result<AccountData<PlaylistSquarePage>, ClientError> {
    client
        .playlist_square_recommendations(&epoch, continuation.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_tag_catalogue(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<PlaylistTagGroup>>, ClientError> {
    client
        .playlist_tag_catalogue(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_tag_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: PlaylistTagQuery,
) -> Result<AccountData<PlaylistTagPage>, ClientError> {
    client
        .playlist_tag_page(&epoch, query)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_subscribers(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    offset: u32,
) -> Result<AccountData<PlaylistSubscribersPage>, ClientError> {
    client
        .playlist_subscribers(&epoch, &id, offset)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn playlist_page(
    client: tauri::State<'_, MusicClient>,
    id: String,
    page: PageRequest,
) -> Result<AccountData<TrackPage>, ClientError> {
    client.playlist_page(&id, page).await.map_err(Into::into)
}
#[tauri::command]
pub async fn search_songs(
    client: tauri::State<'_, MusicClient>,
    keyword: String,
    page: PageRequest,
) -> Result<AccountData<TrackPage>, ClientError> {
    client
        .search_songs(&keyword, page)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn daily_songs(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<TrackPage>, ClientError> {
    client.daily_songs().await.map_err(Into::into)
}
#[tauri::command]
pub async fn style_recommendations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<TrackPage>, ClientError> {
    client
        .style_recommendations(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn style_home_seeds(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<cumulus_core::client::style_home::StyleSeedPage>, ClientError> {
    client.style_home_seeds(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn seeded_style_recommendations(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    seed: cumulus_core::client::style_home::StyleSeedHandle,
) -> Result<AccountData<TrackPage>, ClientError> {
    client
        .seeded_style_recommendations(&epoch, seed)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn liked_ids(
    client: tauri::State<'_, MusicClient>,
) -> Result<AccountData<Vec<String>>, ClientError> {
    client.liked_ids().await.map_err(Into::into)
}
#[tauri::command]
pub async fn set_liked(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    liked: bool,
) -> Result<AccountData<LikeResult>, ClientError> {
    client
        .set_liked(&epoch, &id, liked)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn like_recovery(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    offset: usize,
) -> Result<AccountData<LikeRecoveryView>, ClientError> {
    client.like_recovery(&epoch, offset).map_err(Into::into)
}
#[tauri::command]
pub async fn recover_like(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: LikeRecoveryCommand,
) -> Result<AccountData<LikeOperationView>, ClientError> {
    client
        .recover_like(&epoch, command)
        .await
        .map_err(Into::into)
}
