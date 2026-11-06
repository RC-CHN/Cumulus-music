//! Main-window typed SongCard picker. The native card resolves the song.
use super::voice_related::require_main;
use cumulus_core::{
    client::{
        ClientError, MusicClient,
        dto::{
            DownloadResult, VoiceSongDownloadCreation, VoiceSongDownloadPicker,
            VoiceSongDownloadPlan,
        },
    },
    download::service::DownloadOptions,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceSongDownloadTarget {
    epoch: String,
    play_id: String,
    receipt: String,
    index: usize,
}

#[tauri::command]
pub async fn player_voice_song_download_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
) -> Result<VoiceSongDownloadPlan, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_prepare(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_start<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    options: DownloadOptions,
) -> Result<DownloadResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_start(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            options,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_choose_playlist<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    playlist_id: String,
) -> Result<VoiceSongDownloadPicker, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_choose_playlist(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            &playlist_id,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_picker_status<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    reconcile: bool,
) -> Result<VoiceSongDownloadPicker, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_picker_status(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            reconcile,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_create_playlist<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    name: String,
    private: bool,
) -> Result<VoiceSongDownloadCreation, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_create_playlist(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            &name,
            private,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_creation_status<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    reconcile: bool,
) -> Result<VoiceSongDownloadCreation, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_creation_status(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            reconcile,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_add_created_playlist<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
) -> Result<VoiceSongDownloadCreation, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_add_created_playlist(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_download_start_from_playlist<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceSongDownloadTarget,
    continuation: String,
    options: DownloadOptions,
) -> Result<DownloadResult, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_download_start_from_playlist(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.index,
            &continuation,
            options,
        )
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::VoiceSongDownloadTarget;
    use serde_json::json;

    #[test]
    fn native_target_never_accepts_display_ids_or_a_renderer_download_id() {
        let valid = json!({"epoch":"2","playId":"voice-play","receipt":"receipt","index":3});
        assert!(serde_json::from_value::<VoiceSongDownloadTarget>(valid.clone()).is_ok());
        for key in ["songId", "voiceId", "trackId", "taskId"] {
            let mut invalid = valid.clone();
            invalid[key] = json!("420");
            assert!(serde_json::from_value::<VoiceSongDownloadTarget>(invalid).is_err());
        }
    }
}
