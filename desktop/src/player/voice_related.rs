use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            AccountData, LikeResult, PlayerView, PlayerVoiceSongsView, VinylVoiceDetailView,
            VinylVoiceRecommendationsView, VoiceSongAction, VoiceSongSelectionAction,
        },
    },
    membership::Quality,
};

pub(super) fn require_main<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("main window required").into())
    }
}

#[tauri::command]
pub async fn player_vinyl_voice_detail<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylVoiceDetailView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_detail(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_vinyl_voice_recommendations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylVoiceRecommendationsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendations(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn player_vinyl_voice_recommendations_next<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
) -> Result<VinylVoiceRecommendationsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendations_next(&epoch, &play_id, &receipt)
        .map_err(Into::into)
}

#[tauri::command]
pub fn player_vinyl_voice_recommendation_target<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    page: usize,
    radio_id: String,
) -> Result<String, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendation_target(&epoch, &play_id, &receipt, page, &radio_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<PlayerVoiceSongsView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_play<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    action: VoiceSongAction,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_play(&epoch, &play_id, &receipt, action, quality)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_selection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    action: VoiceSongSelectionAction,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_selection(&epoch, &play_id, &receipt, action, quality)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_like<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    index: usize,
    liked: bool,
) -> Result<AccountData<LikeResult>, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_like(&epoch, &play_id, &receipt, index, liked)
        .await
        .map_err(Into::into)
}
