use super::*;
use cumulus_core::client::{
    ClientError,
    mini::{MiniIntelligenceCommand, MiniPlaybackReceipt},
};

#[tauri::command]
pub async fn mini_intelligence<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    command: MiniIntelligenceCommand,
) -> std::result::Result<MiniPlaybackReceipt, ClientError> {
    let operation = command.operation_id().to_owned();
    if operation.len() > 64 {
        return Err(Error::InvalidInput("invalid intelligence operation id").into());
    }
    let host = app.state::<Host>();
    let snapshot = {
        let mut ledger = host.lock()?;
        let snapshot = ledger.captured(&generation, &ticket)?;
        let window = ledger.get_mut(&generation)?;
        if window.intelligence.is_some() {
            return Err(Error::MutationBusy.into());
        }
        window.intelligence = Some((snapshot.view.epoch.clone(), operation.clone()));
        snapshot
    };
    // Feature HTTP must not hold the window's transport/settings mutex.
    let result = app
        .state::<MusicClient>()
        .mini_intelligence(&snapshot, command)
        .await;
    if let Ok(mut ledger) = host.lock()
        && let Ok(window) = ledger.get_mut(&generation)
        && window
            .intelligence
            .as_ref()
            .is_some_and(|(_, id)| id == &operation)
    {
        window.intelligence = None;
    }
    result.map_err(Into::into)
}

#[tauri::command]
pub async fn mini_mode<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    mode: cumulus_core::playback::PlaybackMode,
) -> std::result::Result<MiniPlaybackReceipt, ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    app.state::<MusicClient>()
        .mini_mode(&snapshot, mode)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn mini_intelligence_cancel<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    epoch: String,
    operation_id: String,
) -> std::result::Result<bool, ClientError> {
    let host = app.state::<Host>();
    let ledger = host.lock()?;
    let window = ledger.get(&generation)?;
    if window.phase != Phase::Active
        || window
            .snapshot
            .as_ref()
            .is_none_or(|s| s.view.epoch != epoch)
    {
        return Err(Error::StaleResponse.into());
    }
    app.state::<MusicClient>()
        .player_intelligence_cancel(&epoch, &operation_id)
        .map_err(Into::into)
}
