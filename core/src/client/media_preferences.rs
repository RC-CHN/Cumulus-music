use super::{MusicClient, dto::AccountData};
use crate::{Error, Result, preferences::system_media::SystemMediaPreferences};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SystemMediaPreferencesView {
    pub revision: Option<String>,
    pub enabled: bool,
    pub damaged: bool,
}
impl From<SystemMediaPreferences> for SystemMediaPreferencesView {
    fn from(value: SystemMediaPreferences) -> Self {
        Self {
            revision: Some(value.revision),
            enabled: value.enabled,
            damaged: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemMediaPreferencesCommand {
    Change {
        expected_revision: String,
        enabled: bool,
    },
    Repair,
}
impl MusicClient {
    /// Native host startup reads only installation-local policy; no player or
    /// account is initialized. UI access still goes through epoch-fenced calls.
    pub fn system_media_preferences_native(&self) -> Result<SystemMediaPreferencesView> {
        match SystemMediaPreferences::read(&self.registry.store()) {
            Ok(value) => Ok(value.into()),
            Err(Error::Json(_)) => Ok(SystemMediaPreferencesView {
                revision: None,
                enabled: false,
                damaged: true,
            }),
            Err(error) => Err(error),
        }
    }
    pub fn system_media_preferences(
        &self,
        epoch: &str,
    ) -> Result<AccountData<SystemMediaPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.system_media_preferences_native()?,
            })
        })
    }
    pub fn system_media_preferences_command(
        &self,
        epoch: &str,
        command: SystemMediaPreferencesCommand,
    ) -> Result<AccountData<SystemMediaPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match command {
                SystemMediaPreferencesCommand::Change {
                    expected_revision,
                    enabled,
                } => SystemMediaPreferences::replace(
                    &self.registry.store(),
                    &expected_revision,
                    enabled,
                )?,
                SystemMediaPreferencesCommand::Repair => {
                    SystemMediaPreferences::repair(&self.registry.store())?
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: value.into(),
            })
        })
    }
}
