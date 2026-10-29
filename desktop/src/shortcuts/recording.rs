//! Temporary key-grab suspension while the main window records a chord.
//! No switch is persisted. Cancellation, navigation and expiry restore the
//! latest committed settings rather than an old in-memory copy.
use super::host;
use cumulus_core::{
    Error, Result,
    client::{ClientError, MusicClient},
};
use tauri::Manager;
fn begin<R: tauri::Runtime>(app: &tauri::AppHandle<R>, epoch: &str) -> Result<String> {
    let host = app
        .try_state::<host::Host>()
        .ok_or(Error::State("shortcut host unavailable"))?;
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    let value = app.state::<MusicClient>().shortcut_settings(epoch)?.value;
    if value.damaged || value.current.is_none() {
        return Err(Error::State("shortcut settings damaged"));
    }
    let id = host::record(&host)?;
    host::apply(app, &value);
    Ok(id)
}
pub(super) fn end<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> Result<()> {
    let Some(host) = app.try_state::<host::Host>() else {
        return Ok(());
    };
    let _operation = host
        .operations
        .lock()
        .map_err(|_| Error::State("shortcut host unavailable"))?;
    if !host::finish_recording(&host, Some(id)) {
        return Ok(());
    }
    match app.state::<MusicClient>().shortcut_settings_native() {
        Ok(value) => host::apply(app, &value),
        Err(error) => {
            host::unavailable(app);
            return Err(error);
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn shortcut_begin_recording<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
) -> std::result::Result<String, ClientError> {
    let worker = app.clone();
    let id = tauri::async_runtime::spawn_blocking(move || begin(&worker, &epoch))
        .await
        .map_err(|_| ClientError::from(Error::State("shortcut worker unavailable")))??;
    let expiry = id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        let _ = tauri::async_runtime::spawn_blocking(move || end(&app, &expiry)).await;
    });
    Ok(id)
}
#[tauri::command]
pub async fn shortcut_end_recording<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> std::result::Result<(), ClientError> {
    tauri::async_runtime::spawn_blocking(move || end(&app, &id))
        .await
        .map_err(|_| ClientError::from(Error::State("shortcut worker unavailable")))?
        .map_err(Into::into)
}
