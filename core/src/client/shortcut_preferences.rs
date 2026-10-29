use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    preferences::shortcuts::{Action, Chord, Scope, ShortcutSettings},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutSettingsView {
    pub current: Option<ShortcutSettings>,
    pub damaged: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShortcutEdit {
    Bind {
        action: Action,
        scope: Scope,
        keys: Vec<u16>,
    },
    Global {
        enabled: bool,
    },
    Media {
        enabled: bool,
    },
    Reset,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShortcutSettingsCommand {
    Change {
        expected_revision: String,
        edit: ShortcutEdit,
    },
    Repair,
}
impl MusicClient {
    /// Independent bootstrap read; neither account HTTP nor audio construction.
    pub fn shortcut_settings_native(&self) -> Result<ShortcutSettingsView> {
        match ShortcutSettings::read(&self.registry.store()) {
            Ok(current) => Ok(ShortcutSettingsView {
                current: Some(current),
                damaged: false,
            }),
            Err(Error::Json(_)) => Ok(ShortcutSettingsView {
                current: None,
                damaged: true,
            }),
            Err(e) => Err(e),
        }
    }
    pub fn shortcut_settings(&self, epoch: &str) -> Result<AccountData<ShortcutSettingsView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.shortcut_settings_native()?,
            })
        })
    }
    pub fn shortcut_settings_command(
        &self,
        epoch: &str,
        command: ShortcutSettingsCommand,
    ) -> Result<AccountData<ShortcutSettingsView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let store = self.registry.store();
            let current = match command {
                ShortcutSettingsCommand::Repair => ShortcutSettings::repair(&store)?,
                ShortcutSettingsCommand::Change {
                    expected_revision,
                    edit,
                } => {
                    let mut value = ShortcutSettings::read(&store)?;
                    if value.revision != expected_revision {
                        return Err(Error::State("shortcut settings changed"));
                    }
                    match edit {
                        ShortcutEdit::Bind {
                            action,
                            scope,
                            keys,
                        } => {
                            let chord = if keys.is_empty() {
                                None
                            } else {
                                Some(Chord::new(keys)?)
                            };
                            value.bind(action, scope, chord)?;
                        }
                        ShortcutEdit::Global { enabled } => value.global_enabled = enabled,
                        ShortcutEdit::Media { enabled } => value.media_enabled = enabled,
                        ShortcutEdit::Reset => value.reset_defaults()?,
                    }
                    value.save(&store)?
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: ShortcutSettingsView {
                    current: Some(current),
                    damaged: false,
                },
            })
        })
    }
}
