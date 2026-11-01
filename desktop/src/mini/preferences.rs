//! Saving a device preference and applying a native window flag are distinct.
use super::*;
use cumulus_core::client::{
    ClientError,
    dto::{MiniPreferencesCommand, MiniPreferencesView},
};

async fn apply<R: Runtime>(
    app: &tauri::AppHandle<R>,
    generation: String,
    epoch: String,
    displayed: MiniPreferencesView,
) -> Result<()> {
    gui(app, move |app| {
        let label = app.state::<Host>().lock()?.get(&generation)?.label.clone();
        let current = app.state::<MusicClient>().mini_preferences(&epoch)?.value;
        if current.damaged || current != displayed {
            return Err(Error::StaleResponse);
        }
        app.get_webview_window(&label)
            .ok_or(Error::StaleResponse)?
            .set_always_on_top(displayed.always_on_top)
            .map_err(|_| Error::State("mini pin failed"))
    })
    .await
}

#[tauri::command]
pub async fn mini_preferences_command<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    command: MiniPreferencesCommand,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    let epoch = snapshot.view.epoch;
    let owner = epoch.clone();
    let worker = app.clone();
    let value = tauri::async_runtime::spawn_blocking(move || {
        worker
            .state::<MusicClient>()
            .mini_preferences_command(&owner, command)
    })
    .await
    .map_err(|_| Error::State("mini settings unavailable"))??;
    apply(&app, generation, epoch, value.value)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn mini_preferences_apply<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation: String,
    ticket: String,
    expected_revision: String,
) -> std::result::Result<(), ClientError> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    let snapshot = host.lock()?.captured(&generation, &ticket)?;
    let epoch = snapshot.view.epoch;
    let value = app.state::<MusicClient>().mini_preferences(&epoch)?.value;
    if value.damaged || value.revision.as_deref() != Some(&expected_revision) {
        return Err(Error::StaleResponse.into());
    }
    // Apply exactly the saved/displayed value; no toggle or database write.
    apply(&app, generation, epoch, value)
        .await
        .map_err(Into::into)
}
