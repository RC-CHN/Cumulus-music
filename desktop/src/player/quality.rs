//! Quality capabilities and trial operations stay native; IPC exposes whitelists.
use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{QualityDetailsView, QualityTrialCommand, QualityTrialOutcome},
};
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
