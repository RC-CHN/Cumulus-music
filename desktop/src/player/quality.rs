//! Quality capabilities and trial operations stay native; IPC exposes whitelists.
use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            PlaybackQualityPreferencesView, QualityDetailsView, QualityTrialCommand,
            QualityTrialOutcome,
        },
    },
    membership::Quality,
};
use serde::{Deserialize, de::DeserializeOwned};
use tauri::ipc::{InvokeBody, Request};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaybackQualitySettingsQuery {
    epoch: String,
    refresh: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaybackQualitySettingsSelection {
    epoch: String,
    revision: String,
    quality: Quality,
}

fn settings_args<R: tauri::Runtime, T: DeserializeOwned>(
    window: &tauri::WebviewWindow<R>,
    request: Request<'_>,
) -> Result<T, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("playback quality settings require main window").into());
    }
    let InvokeBody::Json(value) = request.body() else {
        return Err(Error::InvalidInput("invalid playback quality settings request").into());
    };
    T::deserialize(value)
        .map_err(|_| Error::InvalidInput("invalid playback quality settings request").into())
}

#[tauri::command]
pub async fn playback_quality_preferences<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<PlaybackQualityPreferencesView, ClientError> {
    let input: PlaybackQualitySettingsQuery = settings_args(&window, request)?;
    client
        .playback_quality_preferences(&input.epoch, input.refresh)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn save_playback_quality_preference<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<PlaybackQualityPreferencesView, ClientError> {
    let input: PlaybackQualitySettingsSelection = settings_args(&window, request)?;
    client
        .save_playback_quality_preference(&input.epoch, &input.revision, input.quality)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_quality_details(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<QualityDetailsView, ClientError> {
    client
        .player_quality_details(&epoch, &play_id)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_quality_trial(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: QualityTrialCommand,
) -> Result<QualityTrialOutcome, ClientError> {
    client
        .player_quality_trial(&epoch, command)
        .await
        .map_err(Into::into)
}
