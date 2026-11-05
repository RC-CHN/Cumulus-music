use cumulus_core::{
    Error,
    client::{ClientError, MusicClient},
    playback::StreamingMediaCacheStatus,
};

fn settings_error(error: Error) -> ClientError {
    match error.root() {
        Error::StaleResponse => ClientError {
            code: "playback_cache_changed",
            message: "缓存设置已变化，请核对当前设置后再试。",
            retryable: false,
        },
        Error::InvalidInput(_) => ClientError {
            code: "playback_cache_capacity",
            message: "请选择 512M 到 10G 之间的缓存上限。",
            retryable: false,
        },
        _ => error.into(),
    }
}

/// Configure the app-owned cache before any native playback controller starts.
/// No renderer input is used here. An unavailable cache leaves playback on
/// the normal path and does not prevent startup.
pub fn configure_streaming_cache(mut client: MusicClient) -> MusicClient {
    if client.configure_streaming_media_cache().is_err() {
        eprintln!("Cumulus playback cache is unavailable.");
    }
    client
}

#[tauri::command]
pub async fn playback_cache_state(
    client: tauri::State<'_, MusicClient>,
) -> Result<Option<StreamingMediaCacheStatus>, ClientError> {
    client
        .streaming_media_cache_status()
        .map_err(settings_error)
}

#[tauri::command]
pub async fn playback_cache_set_capacity(
    client: tauri::State<'_, MusicClient>,
    expected_revision: String,
    capacity_bytes: u64,
) -> Result<StreamingMediaCacheStatus, ClientError> {
    client
        .set_streaming_media_cache_capacity(&expected_revision, capacity_bytes)
        .map_err(settings_error)
}

#[tauri::command]
pub async fn playback_cache_clear(
    client: tauri::State<'_, MusicClient>,
    expected_revision: String,
) -> Result<StreamingMediaCacheStatus, ClientError> {
    client
        .clear_streaming_media_cache_checked(&expected_revision)
        .map_err(settings_error)
}
