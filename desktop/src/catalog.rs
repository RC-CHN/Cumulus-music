use cumulus_core::catalog::vertical_zone::{VerticalZoneMenu, VerticalZonePage, ZoneEntry};
use cumulus_core::client::{ClientError, MusicClient, dto::*};

fn require_main(label: &str) -> Result<(), ClientError> {
    if label != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "vertical zone navigation requires the main window",
        )
        .into());
    }
    Ok(())
}

#[tauri::command]
pub async fn vertical_zone_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<VerticalZoneMenu>, ClientError> {
    require_main(window.label())?;
    client.vertical_zone_menu(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn vertical_zone_page<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    page_code: String,
    tab_key: Option<String>,
    tab_key_name: Option<String>,
) -> Result<AccountData<VerticalZonePage>, ClientError> {
    require_main(window.label())?;
    let entry = match (tab_key.as_deref(), tab_key_name.as_deref()) {
        (None, Some(name)) => ZoneEntry::SearchTabName(name),
        (Some(_), Some(_)) => {
            return Err(cumulus_core::Error::InvalidInput(
                "vertical zone tab key and display name are mutually exclusive",
            )
            .into());
        }
        (key, None) => ZoneEntry::Tab(key.unwrap_or_default()),
    };
    client
        .vertical_zone_page(&epoch, &page_code, entry)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn vertical_zone_playlist_origin<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    view_id: String,
    card_key: String,
    id: String,
) -> Result<AccountData<ZonePlaylistOrigin>, ClientError> {
    require_main(window.label())?;
    client
        .vertical_zone_playlist_origin(&epoch, &view_id, &card_key, &id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn vertical_zone_playlist_detail<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    id: String,
    page: PageRequest,
) -> Result<AccountData<TrackPage>, ClientError> {
    require_main(window.label())?;
    client
        .vertical_zone_playlist_detail(&epoch, &receipt, &id, page)
        .await
        .map_err(Into::into)
}

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

#[tauri::command]
pub async fn discovery_home(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<cumulus_core::client::discovery_home::DiscoveryHomePage>, ClientError> {
    client.discovery_home(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn discovery_home_rankings(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<cumulus_core::catalog::rankings::RankingGroup>>, ClientError> {
    client
        .discovery_home_rankings(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn ordinary_essential(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    category: u8,
) -> Result<AccountData<OrdinaryEssentialView>, ClientError> {
    client
        .ordinary_essential(&epoch, category)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn ordinary_essential_books(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    category: u8,
) -> Result<AccountData<Option<EssentialBooks>>, ClientError> {
    client
        .ordinary_essential_books(&epoch, category)
        .await
        .map_err(Into::into)
}
