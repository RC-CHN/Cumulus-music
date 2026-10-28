use super::MusicClient;
use super::dto::AccountData;
use crate::{
    Error, Result,
    preferences::window::{WindowPreferences, WindowSettings},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct WindowPreferencesView {
    pub revision: Option<String>,
    pub settings: WindowSettings,
    pub damaged: bool,
}
impl From<WindowPreferences> for WindowPreferencesView {
    fn from(value: WindowPreferences) -> Self {
        Self {
            revision: Some(value.revision.to_string()),
            settings: value.settings,
            damaged: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WindowPreferencesCommand {
    Change {
        expected_revision: String,
        settings: WindowSettings,
    },
    Repair,
}
impl MusicClient {
    pub fn window_preferences(&self, epoch: &str) -> Result<AccountData<WindowPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match WindowPreferences::read(&self.registry.store()) {
                Ok(value) => value.into(),
                Err(Error::Json(_)) => WindowPreferencesView {
                    revision: None,
                    settings: Default::default(),
                    damaged: true,
                },
                Err(error) => return Err(error),
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
    pub fn window_preferences_command(
        &self,
        epoch: &str,
        command: WindowPreferencesCommand,
    ) -> Result<AccountData<WindowPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match command {
                WindowPreferencesCommand::Change {
                    expected_revision,
                    settings,
                } => {
                    let revision = expected_revision.parse::<u64>().map_err(|_| {
                        Error::InvalidInput("invalid window behavior settings revision")
                    })?;
                    if revision.to_string() != expected_revision {
                        return Err(Error::InvalidInput(
                            "invalid window behavior settings revision",
                        ));
                    }
                    WindowPreferences::replace(&self.registry.store(), revision, settings)?
                }
                WindowPreferencesCommand::Repair => {
                    WindowPreferences::repair(&self.registry.store())?
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: value.into(),
            })
        })
    }
}
