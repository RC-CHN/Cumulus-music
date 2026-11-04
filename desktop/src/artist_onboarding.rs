use cumulus_core::{
    client::{
        ClientError, MusicClient,
        artist_onboarding::{ArtistCandidate, ArtistOffer, ArtistResultAction, ArtistSave},
        dto::{AccountData, PlayerView, TrackView},
    },
    membership::Quality,
};

#[tauri::command]
pub fn artist_onboarding_offer(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Option<ArtistOffer>>, ClientError> {
    client.artist_onboarding_offer(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn artist_onboarding_candidates(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    similar_to: Option<String>,
) -> Result<AccountData<Vec<ArtistCandidate>>, ClientError> {
    client
        .artist_onboarding_candidates(&epoch, &id, similar_to.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn artist_onboarding_save(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    artists: Vec<String>,
) -> Result<AccountData<ArtistSave>, ClientError> {
    client
        .artist_onboarding_save(&epoch, &id, artists)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn artist_onboarding_preview(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<AccountData<Vec<TrackView>>, ClientError> {
    client
        .artist_onboarding_preview(&epoch, &id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn artist_onboarding_close(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
) -> Result<(), ClientError> {
    client
        .artist_onboarding_close(&epoch, &id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn artist_onboarding_result(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: String,
    action: ArtistResultAction,
    queue_revision: String,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    client
        .artist_onboarding_result(&epoch, &id, action, queue_revision, quality)
        .await
        .map_err(Into::into)
}
