use super::super::dto::AccountData;
use super::MusicClient;
use crate::{
    Error, Result,
    playback::preferences::{StartupPreferences, StartupSettings},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct StartupPreferencesView {
    pub revision: Option<String>,
    pub settings: StartupSettings,
    pub damaged: bool,
}
impl From<StartupPreferences> for StartupPreferencesView {
    fn from(value: StartupPreferences) -> Self {
        Self {
            revision: Some(value.revision.to_string()),
            settings: value.settings,
            damaged: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StartupPreferencesCommand {
    Change {
        expected_revision: String,
        settings: StartupSettings,
    },
    Repair,
}
impl MusicClient {
    pub fn startup_preferences(&self, epoch: &str) -> Result<AccountData<StartupPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match StartupPreferences::read(&self.player.store) {
                Ok(value) => value.into(),
                Err(Error::Json(_)) => StartupPreferencesView {
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
    pub fn startup_preferences_command(
        &self,
        epoch: &str,
        command: StartupPreferencesCommand,
    ) -> Result<AccountData<StartupPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match command {
                StartupPreferencesCommand::Change {
                    expected_revision,
                    settings,
                } => {
                    let revision = expected_revision
                        .parse::<u64>()
                        .map_err(|_| Error::InvalidInput("invalid playback settings revision"))?;
                    if revision.to_string() != expected_revision {
                        return Err(Error::InvalidInput("invalid playback settings revision"));
                    }
                    StartupPreferences::replace(&self.player.store, revision, settings)?
                }
                StartupPreferencesCommand::Repair => {
                    StartupPreferences::repair(&self.player.store)?
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: value.into(),
            })
        })
    }
}
