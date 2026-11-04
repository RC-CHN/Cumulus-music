//! M2157, M622 and M55: moderation of a visible top-level event card.
use super::{AccountData, Locator, MusicClient, handle, project};
use crate::{Error, Result, account::SessionToken, error::code_matches};
use serde_json::json;
use uuid::Uuid;

use super::super::{
    profile_report::ProfileReportReason,
    report_receipt::{ReportCommand, ReportJournal, ReportStatus, ReportView},
};

pub type CommunityReportCommand = ReportCommand<ProfileReportReason>;
pub type CommunityReportView = ReportView;

impl Locator {
    pub(super) fn reportable(&self, viewer: &str) -> bool {
        !self.event.starts_with('0')
            && !self.owner.starts_with('0')
            && self.owner != viewer.trim_start_matches('0')
    }
}

impl MusicClient {
    fn note_report_locator(
        &self,
        token: &SessionToken,
        key: Uuid,
        viewer: &str,
    ) -> Result<Locator> {
        self.community_host_for(token, key)?
            .state(token)?
            .cards
            .iter()
            .find(|row| row.key == key)
            .and_then(|row| row.locator.clone())
            .filter(|locator| locator.reportable(viewer))
            .ok_or(Error::StaleResponse)
    }

    pub async fn community_note_report(
        &self,
        epoch: &str,
        detail: &str,
        command: CommunityReportCommand,
    ) -> Result<AccountData<CommunityReportView>> {
        let token = self.playlist_token(epoch)?;
        let key = handle(detail)?;
        let locator = self.session.with_token(&token, |viewer| {
            self.note_report_locator(&token, key, viewer)
        })?;
        let journal = ReportJournal {
            session: &self.session,
            token: &token,
            store: self.session.persistent_store()?,
            kind: "community-event-report-v1",
            lock_prefix: "community-event-report",
            key: locator.event.clone(),
            draft_scope: Some(format!("{}:{}:{key}", locator.event, locator.owner)),
        };
        let value = self.community_report.act(&journal, command, ProfileReportReason::into_wire, |reason| {
            let journal = &journal;
            let token = &token;
            async move {
                self.session.run_for(token, |api, _| async move {
                    let path = format!("/api/v1/event/{}/{}", locator.owner, locator.event);
                    let detail = api.call(&path, json!({}), false, &[200, 403, 404, 410]).await?;
                    if !code_matches(&detail["code"], 200)
                        || project::card(&detail["event"], true).and_then(|(_, target, _)| target).as_ref() != Some(&locator)
                    {
                        return Ok(ReportStatus::TargetUnavailable);
                    }
                    self.session.with_token(token, |viewer| {
                        if self.note_report_locator(token, key, viewer)? != locator {
                            return Err(Error::StaleResponse);
                        }
                        Ok(())
                    })?;
                    journal.send(&api, "/api/report/reportevent", json!({"id":locator.event,"userid":locator.owner,"reason":reason}), false).await
                }).await
            }
        }).await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
