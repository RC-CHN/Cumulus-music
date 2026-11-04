use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::PlaybackPowerView},
};

fn require_main(label: &str) -> Result<(), ClientError> {
    if label != "main" {
        return Err(Error::InvalidInput("playback power settings require the main window").into());
    }
    Ok(())
}

#[tauri::command]
pub async fn playback_power<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<PlaybackPowerView, ClientError> {
    require_main(window.label())?;
    client.playback_power(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn set_playback_power<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    enabled: bool,
) -> Result<PlaybackPowerView, ClientError> {
    require_main(window.label())?;
    client
        .set_playback_power(&epoch, enabled)
        .await
        .map_err(Into::into)
}
