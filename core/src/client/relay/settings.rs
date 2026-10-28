use super::super::{MusicClient, dto::AccountData};
use super::RelayView;
use crate::relay::RelayAvailability;
use crate::{
    Result,
    account::{SessionToken, settings::SettingsPhase},
    relay::{RelaySettingIntent, RelaySettings},
};
use serde::{Deserialize, Serialize};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySettingsView {
    pub revision: String,
    pub enabled: Option<bool>,
    pub can_change: bool,
    pub operations: Vec<RelaySettingOperationView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySettingOperationView {
    pub id: String,
    pub enabled: bool,
    pub before: Option<bool>,
    pub observed: Option<bool>,
    pub phase: SettingsPhase,
    pub created_at: u64,
    pub last_read_at: Option<u64>,
    pub read_revision: Option<String>,
}
impl From<RelaySettingIntent> for RelaySettingOperationView {
    fn from(v: RelaySettingIntent) -> Self {
        Self {
            id: v.id,
            enabled: v.enabled,
            before: v.before,
            observed: v.observed,
            phase: v.phase,
            created_at: v.created_at,
            last_read_at: v.last_read_at,
            read_revision: v.read_revision,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelaySettingCommand {
    SettingChange {
        id: String,
        revision: String,
        enabled: bool,
    },
    SettingCancelUnseen {
        id: String,
        revision: String,
        enabled: bool,
    },
    SettingReconcile {
        id: String,
    },
    SettingAccept {
        id: String,
        revision: String,
    },
}
impl MusicClient {
    pub(super) fn relay_setting_view(
        &self,
        token: &SessionToken,
        availability: RelayAvailability,
        requested: Option<&str>,
    ) -> Result<Option<RelaySettingsView>> {
        let service = RelaySettings::new(self.session.clone())?;
        let Some(snapshot) = service.snapshot_for(token)? else {
            return Ok(None);
        };
        let mut operations = service.operations_for(token)?;
        let pending = operations.iter().any(|op| op.phase.unresolved());
        operations.sort_by_key(|op| {
            (
                std::cmp::Reverse(op.phase.unresolved()),
                std::cmp::Reverse(op.created_at),
            )
        });
        let requested = requested
            .and_then(|id| operations.iter().find(|op| op.id == id))
            .cloned();
        operations.truncate(20);
        if let Some(op) = requested
            && !operations.iter().any(|v| v.id == op.id)
        {
            if operations.len() == 20 {
                operations.pop();
            }
            operations.push(op);
        }
        Ok(Some(RelaySettingsView {
            revision: snapshot.revision,
            enabled: snapshot.enabled,
            can_change: !pending
                && snapshot.enabled.is_some()
                && matches!(
                    availability,
                    RelayAvailability::Available | RelayAvailability::ServerDisabled
                ),
            operations: operations.into_iter().map(Into::into).collect(),
        }))
    }
    pub async fn relay_setting_command(
        &self,
        epoch: &str,
        command: RelaySettingCommand,
    ) -> Result<AccountData<RelayView>> {
        let token = self.player_token(epoch)?;
        let service = RelaySettings::new(self.session.clone())?;
        let op = match command {
            RelaySettingCommand::SettingChange {
                id,
                revision,
                enabled,
            } => service.change_for(&token, &id, &revision, enabled).await?,
            RelaySettingCommand::SettingCancelUnseen {
                id,
                revision,
                enabled,
            } => service.cancel_unseen_for(&token, &id, &revision, enabled)?,
            RelaySettingCommand::SettingReconcile { id } => {
                service.reconcile_for(&token, &id).await?
            }
            RelaySettingCommand::SettingAccept { id, revision } => {
                service.accept_current_for(&token, &id, &revision).await?
            }
        };
        // Official doInit rereads configuration after the device switch. If it
        // fails, the caller reads the durable receipt; no settings write repeats.
        let mut page = self.relay_view(epoch, true).await?;
        page.value.settings =
            self.relay_setting_view(&token, page.value.availability, Some(&op.id))?;
        Ok(page)
    }
}
