use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    preferences::desktop_lyrics::{DesktopLyricPreferences, Edit},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricPreferencesView {
    pub current: Option<DesktopLyricPreferences>,
    pub damaged: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopLyricPreferencesCommand {
    Change {
        expected_revision: String,
        edit: Box<Edit>,
    },
    Repair,
}
impl MusicClient {
    pub fn desktop_lyric_preferences_native(&self) -> Result<DesktopLyricPreferencesView> {
        match DesktopLyricPreferences::read(&self.registry.store()) {
            Ok(current) => Ok(DesktopLyricPreferencesView {
                current: Some(current),
                damaged: false,
            }),
            Err(Error::Json(_)) => Ok(DesktopLyricPreferencesView {
                current: None,
                damaged: true,
            }),
            Err(error) => Err(error),
        }
    }
    pub fn desktop_lyric_preferences(
        &self,
        epoch: &str,
    ) -> Result<AccountData<DesktopLyricPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.desktop_lyric_preferences_native()?,
            })
        })
    }
    pub fn desktop_lyric_preferences_command(
        &self,
        epoch: &str,
        command: DesktopLyricPreferencesCommand,
    ) -> Result<AccountData<DesktopLyricPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match command {
                DesktopLyricPreferencesCommand::Change {
                    expected_revision,
                    edit,
                } => DesktopLyricPreferences::change(
                    &self.registry.store(),
                    &expected_revision,
                    *edit,
                )?,
                DesktopLyricPreferencesCommand::Repair => {
                    DesktopLyricPreferences::repair(&self.registry.store())?
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: DesktopLyricPreferencesView {
                    current: Some(value),
                    damaged: false,
                },
            })
        })
    }
}
