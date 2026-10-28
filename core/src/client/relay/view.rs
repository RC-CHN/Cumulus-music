use super::super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    relay::{PullPhase, RelayAvailability, RelayPlayback, RelayPlaybackPhase, RelayService},
};
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayView {
    pub revision: String,
    pub availability: RelayAvailability,
    pub setting: Option<bool>,
    pub settings: Option<super::RelaySettingsView>,
    pub alert: Option<RelayAlertView>,
    pub operations: Vec<RelayOperationView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayAlertView {
    pub title: String,
    pub device_name: String,
    pub resource_name: String,
    pub button_text: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayOperationView {
    pub id: String,
    pub phase: RelayPlaybackPhase,
    pub pull_phase: Option<PullPhase>,
    pub can_retry: bool,
    pub superseded: bool,
    pub created_at: u64,
}
impl MusicClient {
    pub async fn relay_view(&self, epoch: &str, refresh: bool) -> Result<AccountData<RelayView>> {
        self.relay_view_with_operation(epoch, refresh, None).await
    }
    pub(super) async fn relay_view_with_operation(
        &self,
        epoch: &str,
        refresh: bool,
        requested: Option<&str>,
    ) -> Result<AccountData<RelayView>> {
        let token = self.player_token(epoch)?;
        let applications = RelayPlayback::new(self.session.clone())?;
        match applications.lock(&token) {
            Ok((_owner, _lock)) => applications.recover(&token)?,
            Err(Error::MutationBusy) => (),
            Err(error) => return Err(error),
        }
        let service = RelayService::new(self.session.clone())?;
        let snapshot = if refresh {
            service.refresh_for(&token).await?
        } else {
            service.snapshot_for(&token)?
        };
        let mut records = applications.records(&token)?;
        records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        let retry_sources = records
            .iter()
            .filter_map(|r| r.previous_id.as_ref())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let requested = requested
            .and_then(|id| records.iter().find(|r| r.id == id))
            .cloned();
        records.truncate(20);
        if let Some(record) = requested
            && !records.iter().any(|r| r.id == record.id)
        {
            if records.len() == 20 {
                records.pop();
            }
            records.push(record);
        }
        let operations = records
            .into_iter()
            .map(|record| {
                let pull = service.intent_for(&token, &record.pull_id)?;
                let pull_phase = pull.as_ref().map(|p| p.phase);
                Ok(RelayOperationView {
                    id: record.id.clone(),
                    phase: record.phase,
                    pull_phase,
                    can_retry: matches!(
                        record.phase,
                        RelayPlaybackPhase::Failed | RelayPlaybackPhase::Uncertain
                    ) && pull_phase == Some(PullPhase::Ready)
                        && !retry_sources.contains(&record.id),
                    created_at: record.created_at,
                    superseded: retry_sources.contains(&record.id),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let value = RelayView {
            revision: snapshot.revision,
            availability: snapshot.availability,
            setting: snapshot.setting,
            settings: self.relay_setting_view(&token, snapshot.availability, None)?,
            alert: snapshot.alert.map(|a| RelayAlertView {
                title: a
                    .text("reconnectTitle")
                    .unwrap_or("继续另一台设备的播放")
                    .into(),
                device_name: a.text("deviceName").unwrap_or("另一台设备").into(),
                resource_name: a.text("resourceName").unwrap_or("上次播放的内容").into(),
                button_text: a.text("btnDesc").unwrap_or("继续播放").into(),
            }),
            operations,
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
