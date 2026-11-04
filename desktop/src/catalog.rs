use cumulus_core::client::{ClientError, MusicClient, dto::*};

#[tauri::command]
pub async fn artist_gallery(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: ArtistGalleryQuery,
) -> Result<AccountData<ArtistGalleryPage>, ClientError> {
    client
        .artist_gallery(&epoch, query)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn catalog_membership(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    resource: CatalogResource,
    action: CatalogMembershipAction,
) -> Result<AccountData<CatalogMembershipView>, ClientError> {
    client
        .catalog_membership(&epoch, resource, action)
        .await
        .map_err(Into::into)
}

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

#[tauri::command]
pub async fn followed_new_song_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    start_timestamp: Option<u64>,
) -> Result<AccountData<FollowNewSongPage>, ClientError> {
    client
        .followed_new_song_page(&epoch, start_timestamp)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn followed_new_song_latest(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<FollowLatest>, ClientError> {
    client
        .followed_new_song_latest(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn release_notification(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    artist: String,
    action: ReleaseNotificationAction,
) -> Result<AccountData<ReleaseNotificationState>, ClientError> {
    client
        .release_notification(&epoch, &artist, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn latest_releases(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: LatestReleaseQuery,
) -> Result<AccountData<LatestReleasePage>, ClientError> {
    client
        .latest_releases(&epoch, query)
        .await
        .map_err(Into::into)
}
