use super::{MusicClient, dto::AccountData};
pub use crate::account::settings::{SettingChange, SettingKey, SettingValue, SettingsPhase};
use crate::{
    Error, Result,
    account::settings::{AccountSettings, SettingsMutation},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsPage {
    pub revision: Option<String>,
    pub fetched_at: Option<u64>,
    pub fields: Vec<AccountSettingField>,
    pub linked_privacy: LinkedPrivacy,
    pub operations: Vec<SettingsOperationView>,
}
#[derive(Serialize)]
pub struct AccountSettingField {
    pub key: SettingKey,
    pub value: Option<SettingValue>,
    /// Missing/unknown server values are shown explicitly, never defaulted.
    pub state: SettingAvailability,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingAvailability {
    Available,
    Missing,
    Unsupported,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedPrivacy {
    pub people_can_see_play_record: Option<bool>,
    pub followed_can_see_play_record: Option<bool>,
    pub social_playlist_setting: Option<u8>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsOperationView {
    pub id: String,
    pub change: SettingChange,
    pub before: Option<SettingValue>,
    pub observed: Option<SettingValue>,
    pub phase: SettingsPhase,
    pub created_at: u64,
    pub last_read_at: Option<u64>,
    pub read_revision: Option<String>,
}
impl From<SettingsMutation> for SettingsOperationView {
    fn from(op: SettingsMutation) -> Self {
        Self {
            id: op.id,
            change: op.change,
            before: op.before,
            observed: op.observed,
            phase: op.phase,
            created_at: op.created_at,
            last_read_at: op.last_read_at,
            read_revision: op.read_revision,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountSettingsCommand {
    Change {
        id: String,
        expected_revision: String,
        change: SettingChange,
    },
    CancelUnseen {
        id: String,
        expected_revision: String,
        change: SettingChange,
    },
    Reconcile {
        id: String,
    },
    AcceptCurrent {
        id: String,
        expected_revision: String,
    },
}
impl MusicClient {
    pub async fn account_settings_page(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<AccountSettingsPage>> {
        let token = self.player_token(epoch)?;
        let service = AccountSettings::new(self.session.clone())?;
        if refresh {
            service.refresh_for(&token).await?;
        }
        let (snapshot, mut operations) = service.inspect_for(&token)?;
        operations.sort_by_key(|op| {
            (
                std::cmp::Reverse(op.phase.unresolved()),
                std::cmp::Reverse(op.created_at),
            )
        });
        operations.truncate(20);
        let values = snapshot
            .as_ref()
            .map(|s| &s.values)
            .unwrap_or(&serde_json::Value::Null);
        let fields = SettingKey::ALL
            .into_iter()
            .map(|key| {
                let raw = values.get(key.key());
                let value = raw.and_then(|v| key.value(v));
                AccountSettingField {
                    key,
                    value,
                    state: if value.is_some() {
                        SettingAvailability::Available
                    } else if raw.is_none() {
                        SettingAvailability::Missing
                    } else {
                        SettingAvailability::Unsupported
                    },
                }
            })
            .collect();
        let value = AccountSettingsPage {
            revision: snapshot.as_ref().map(|s| s.revision.clone()),
            fetched_at: snapshot.as_ref().map(|s| s.fetched_at),
            fields,
            linked_privacy: LinkedPrivacy {
                people_can_see_play_record: values["allowPeopleCanSeeMyPlayRecord"].as_bool(),
                followed_can_see_play_record: values["allowFollowedCanSeeMyPlayRecord"].as_bool(),
                social_playlist_setting: values["socialPlaylistSetting"]
                    .as_u64()
                    .filter(|n| *n <= 2)
                    .map(|n| n as u8),
            },
            operations: operations.into_iter().map(Into::into).collect(),
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
    pub async fn account_settings_command(
        &self,
        epoch: &str,
        command: AccountSettingsCommand,
    ) -> Result<AccountData<AccountSettingsPage>> {
        let token = self.player_token(epoch)?;
        if token.uid.is_none() {
            return Err(Error::State("no authenticated account"));
        }
        let service = AccountSettings::new(self.session.clone())?;
        let operation = match command {
            AccountSettingsCommand::Change {
                id,
                expected_revision,
                change,
            } => {
                service
                    .change_for(&token, &id, &expected_revision, change)
                    .await?
            }
            AccountSettingsCommand::CancelUnseen {
                id,
                expected_revision,
                change,
            } => service.cancel_unseen_for(&token, &id, &expected_revision, change)?,
            AccountSettingsCommand::Reconcile { id } => service.reconcile_for(&token, &id).await?,
            AccountSettingsCommand::AcceptCurrent {
                id,
                expected_revision,
            } => {
                service
                    .accept_current_for(&token, &id, &expected_revision)
                    .await?
            }
        };
        let mut result = self.account_settings_page(epoch, false).await?;
        // A delayed IPC recovery may refer to an older terminal intent. Keep
        // its actual result even when newer operations fill the bounded page.
        if !result
            .value
            .operations
            .iter()
            .any(|op| op.id == operation.id)
        {
            if result.value.operations.len() == 20 {
                result.value.operations.pop();
            }
            result.value.operations.push(operation.into());
        }
        Ok(result)
    }
}
