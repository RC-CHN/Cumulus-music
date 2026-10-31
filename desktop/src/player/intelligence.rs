use cumulus_core::client::{
    ClientError, MusicClient,
    dto::{IntelligenceCommand, IntelligenceEligibility, PlayerView},
};

#[tauri::command]
pub async fn player_intelligence(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: IntelligenceCommand,
) -> Result<PlayerView, ClientError> {
    client
        .player_intelligence(&epoch, command)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn player_intelligence_cancel(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    operation_id: String,
) -> Result<bool, ClientError> {
    client
        .player_intelligence_cancel(&epoch, &operation_id)
        .map_err(Into::into)
}
#[tauri::command]
pub async fn player_intelligence_eligibility(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    revision: String,
) -> Result<IntelligenceEligibility, ClientError> {
    client
        .player_intelligence_eligibility(&epoch, &revision)
        .await
        .map_err(Into::into)
}
