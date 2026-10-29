//! Capture the destination lifetime before asynchronous shortcut dispatch.
use super::*;

#[derive(Clone)]
pub(crate) struct ModeIntent {
    epoch: String,
    window: state::Intent,
}
impl ModeIntent {
    pub(crate) fn epoch(&self) -> String {
        self.epoch.clone()
    }
    pub(super) fn check<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<()> {
        if app.state::<MusicClient>().account()?.epoch != self.epoch {
            return Err(Error::StaleResponse);
        }
        app.state::<Host>().lock()?.check_intent(&self.window)
    }
}
pub(crate) fn capture_mode<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<ModeIntent> {
    let epoch = app.state::<MusicClient>().account()?.epoch;
    let window = app.state::<Host>().lock()?.intent();
    let result = ModeIntent { epoch, window };
    result.check(app)?;
    Ok(result)
}
pub(crate) async fn switch_mode<R: Runtime>(
    app: &tauri::AppHandle<R>,
    intent: ModeIntent,
) -> Result<()> {
    if intent.window.is_mini() {
        gui(app, move |app| {
            intent.check(&app)?;
            if !dismiss(&app, None, true) {
                return Err(Error::State("main window unavailable"));
            }
            crate::lifecycle::show(&app);
            Ok(())
        })
        .await
    } else {
        enter_expected(app, Some(intent)).await
    }
}
