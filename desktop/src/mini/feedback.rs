use super::*;
use cumulus_core::{
    client::{ClientError, mini::MiniFmFeedbackView},
    fm::FmReasons,
};

#[tauri::command]
pub async fn mini_fm_feedback_view<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    refresh: bool,
) -> std::result::Result<MiniFmFeedbackView, ClientError> {
    let snapshot = app.state::<Host>().lock()?.captured(&generation, &ticket)?;
    app.state::<MusicClient>()
        .mini_fm_feedback_view(&snapshot, refresh)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn mini_fm_feedback_reasons<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
) -> std::result::Result<FmReasons, ClientError> {
    let snapshot = app.state::<Host>().lock()?.captured(&generation, &ticket)?;
    app.state::<MusicClient>()
        .mini_fm_feedback_reasons(&snapshot)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn mini_fm_feedback<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    reason_key: Option<String>,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    app.state::<MusicClient>()
        .mini_fm_feedback(&snapshot, reason_key)
        .await
        .map_err(Into::into)
}
