use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::VinylVoiceRecommendationsView},
};

fn require_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("main window required").into())
    }
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
