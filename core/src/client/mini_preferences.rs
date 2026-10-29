use super::{MusicClient, dto::AccountData};
use crate::{Error, Result, preferences::mini::MiniPreferences};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniPreferencesView {
    pub revision: Option<String>,
    pub always_on_top: bool,
    pub damaged: bool,
}
impl From<MiniPreferences> for MiniPreferencesView {
    fn from(value: MiniPreferences) -> Self {
        Self {
            revision: Some(value.revision),
            always_on_top: value.always_on_top,
            damaged: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MiniPreferencesCommand {
    Change {
        expected_revision: String,
        always_on_top: bool,
    },
    Repair,
}
impl MusicClient {
    /// Native host startup reads only installation-local policy; no player or
    /// account is initialized. UI access still goes through epoch-fenced calls.
    pub fn mini_preferences_native(&self) -> Result<MiniPreferencesView> {
        match MiniPreferences::read(&self.registry.store()) {
            Ok(value) => Ok(value.into()),
            Err(Error::Json(_)) => Ok(MiniPreferencesView {
                revision: None,
                always_on_top: false,
                damaged: true,
            }),
            Err(error) => Err(error),
        }
    }
    pub fn mini_preferences(&self, epoch: &str) -> Result<AccountData<MiniPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.mini_preferences_native()?,
            })
        })
    }
    pub fn mini_preferences_command(
        &self,
        epoch: &str,
        command: MiniPreferencesCommand,
    ) -> Result<AccountData<MiniPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match command {
                MiniPreferencesCommand::Change {
                    expected_revision,
                    always_on_top,
                } => MiniPreferences::replace(
                    &self.registry.store(),
                    &expected_revision,
                    always_on_top,
                )?,
                MiniPreferencesCommand::Repair => MiniPreferences::repair(&self.registry.store())?,
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: value.into(),
            })
        })
    }
}
