use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient, VinylRelatedPlaylistsView, VinylSimilarAction,
        VinylSimilarSongsView, dto::PlayerView,
    },
    membership::Quality,
};

fn require_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("main window required").into())
    }
}

#[tauri::command]
pub async fn player_vinyl_similar_available<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<bool, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_similar_available(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_vinyl_similar_songs<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylSimilarSongsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_similar_songs(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_vinyl_related_playlists<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylRelatedPlaylistsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_related_playlists(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_vinyl_similar_play<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    action: VinylSimilarAction,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_similar_play(&epoch, &play_id, &receipt, action, quality)
        .await
        .map_err(Into::into)
}
