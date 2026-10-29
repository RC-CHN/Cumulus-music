use super::*;
use cumulus_core::{
    client::dto::DesktopLyricPreferencesCommand, preferences::desktop_lyrics::Edit,
};
pub(crate) struct Intent {
    epoch: String,
    revision: String,
    enabled: bool,
    serial: u64,
    window: Option<String>,
}
impl Intent {
    pub(crate) fn epoch(&self) -> &str {
        &self.epoch
    }
}
pub(crate) fn capture<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Intent> {
    let client = app.state::<MusicClient>();
    let epoch = client.account()?.epoch;
    let value = client
        .desktop_lyric_preferences(&epoch)?
        .value
        .current
        .ok_or(Error::State("desktop lyric settings damaged"))?;
    let host = app.state::<Host>();
    let state = host.lock()?;
    Ok(Intent {
        epoch,
        revision: value.revision,
        enabled: value.enabled,
        serial: state.serial,
        window: state.window.as_ref().map(|w| w.generation.clone()),
    })
}
pub(crate) async fn toggle<R: Runtime>(app: &tauri::AppHandle<R>, intent: Intent) -> Result<()> {
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    {
        let state = host.lock()?;
        if state.serial != intent.serial
            || state.window.as_ref().map(|w| &w.generation) != intent.window.as_ref()
        {
            return Err(Error::StaleResponse);
        }
    }
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        worker
            .state::<MusicClient>()
            .desktop_lyric_preferences_command(
                &intent.epoch,
                DesktopLyricPreferencesCommand::Change {
                    expected_revision: intent.revision,
                    edit: Box::new(Edit::Enabled {
                        value: !intent.enabled,
                    }),
                },
            )
    })
    .await
    .map_err(|_| Error::State("desktop lyric settings unavailable"))??;
    settings::apply(app, false).await
}
