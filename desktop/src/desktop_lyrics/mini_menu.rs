//! The mini window can change only visibility/lock, against a captured local
//! preference revision and native lifetime. It cannot edit styles or geometry.
use super::*;
use cumulus_core::{
    client::dto::DesktopLyricPreferencesCommand, preferences::desktop_lyrics::Edit,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    epoch: String,
    revision: Option<String>,
    serial: u64,
    window: Option<String>,
    view: View,
}
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    pub enabled: bool,
    pub visible: bool,
    pub opening: bool,
    pub locked: bool,
    pub damaged: bool,
    pub error: Option<&'static str>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Toggle,
    Retry,
    Repair,
    Lock,
    Unlock,
}
impl Snapshot {
    pub fn same(&self, other: &Self) -> bool {
        self == other
    }
    pub fn view(&self) -> View {
        self.view.clone()
    }
    pub fn capture<R: Runtime>(app: &tauri::AppHandle<R>, epoch: &str) -> Result<Self> {
        let client = app.state::<MusicClient>();
        let saved = client.desktop_lyric_preferences(epoch)?.value;
        let host = app.state::<Host>();
        let state = host.lock()?;
        let window = state.window.as_ref();
        let view = View {
            enabled: saved.current.as_ref().is_some_and(|v| v.enabled),
            visible: window.is_some_and(|w| w.ready),
            opening: window.is_some_and(|w| !w.ready),
            locked: window.is_some_and(|w| w.locked),
            damaged: saved.damaged,
            error: state.error,
        };
        let snapshot = Self {
            epoch: epoch.into(),
            revision: saved.current.map(|v| v.revision),
            serial: state.serial,
            window: window.map(|w| w.generation.clone()),
            view,
        };
        drop(state);
        if client.account()?.epoch != epoch {
            return Err(Error::StaleResponse);
        }
        Ok(snapshot)
    }
    fn check<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<()> {
        let current = Self::capture(app, &self.epoch)?;
        if current.revision != self.revision
            || current.serial != self.serial
            || current.window != self.window
            || current.view != self.view
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    pub async fn apply<R: Runtime>(self, app: &tauri::AppHandle<R>, action: Action) -> Result<()> {
        let host = app.state::<Host>();
        let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
        self.check(app)?;
        match action {
            Action::Lock | Action::Unlock => {
                if !self.view.visible || matches!(action, Action::Lock) == self.view.locked {
                    return Err(Error::StaleResponse);
                }
                gui(app, move |app| {
                    self.check(&app)?;
                    if matches!(action, Action::Lock) {
                        lock(&app)
                    } else {
                        unlock(&app)
                    }
                })
                .await
            }
            Action::Toggle | Action::Repair | Action::Retry => {
                let change = match action {
                    Action::Repair if self.view.damaged => {
                        Some(DesktopLyricPreferencesCommand::Repair)
                    }
                    Action::Toggle if !self.view.damaged => {
                        Some(DesktopLyricPreferencesCommand::Change {
                            expected_revision: self.revision.clone().ok_or(Error::StaleResponse)?,
                            edit: Box::new(Edit::Enabled {
                                value: !self.view.enabled,
                            }),
                        })
                    }
                    Action::Retry
                        if !self.view.damaged
                            && self.view.enabled
                            && !self.view.visible
                            && !self.view.opening =>
                    {
                        None
                    }
                    _ => return Err(Error::StaleResponse),
                };
                if let Some(change) = change {
                    let worker = app.clone();
                    let epoch = self.epoch;
                    tauri::async_runtime::spawn_blocking(move || {
                        worker
                            .state::<MusicClient>()
                            .desktop_lyric_preferences_command(&epoch, change)
                    })
                    .await
                    .map_err(|_| Error::State("desktop lyric settings unavailable"))??;
                }
                if let Err(error) = settings::apply(app, false).await {
                    host.lock()?.error = Some("设置已保存，但歌词窗口未能应用；请重试打开");
                    return Err(error);
                }
                Ok(())
            }
        }
    }
}
