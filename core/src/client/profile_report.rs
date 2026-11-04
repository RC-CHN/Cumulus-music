//! M622 and M55: a report from an authenticated guest user's PC profile.
use super::{
    MusicClient,
    dto::AccountData,
    report_receipt::{ReportCommand, ReportJournal, ReportStatus, custom_reason},
};
use crate::{
    Error, Result,
    error::code_matches,
    protocol::{api::identifier, wire::serialization::js_truthy},
    storage::OperationLock,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileReportReason {
    Advertising,
    PersonalAttack,
    HarmfulContent,
    Political,
    Other { description: String },
}
impl ProfileReportReason {
    pub(super) fn into_wire(self) -> Result<String> {
        Ok(match self {
            Self::Advertising => "广告或营销信息".into(),
            Self::PersonalAttack => "恶意人身攻击".into(),
            Self::HarmfulContent => "色情、低俗或有害内容".into(),
            Self::Political => "政治敏感言论".into(),
            Self::Other { description } => return custom_reason(description),
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileReportCommand {
    Inspect,
    Prepare { reason: ProfileReportReason },
    Cancel { receipt: String },
    Submit { receipt: String },
}
impl From<ProfileReportCommand> for ReportCommand<ProfileReportReason> {
    fn from(value: ProfileReportCommand) -> Self {
        match value {
            ProfileReportCommand::Inspect => Self::Inspect,
            ProfileReportCommand::Prepare { reason } => Self::Prepare { reason },
            ProfileReportCommand::Cancel { receipt } => Self::Cancel { receipt },
            ProfileReportCommand::Submit { receipt } => Self::Submit { receipt },
        }
    }
}
pub type ProfileReportView = super::report_receipt::ReportView;

impl MusicClient {
    pub async fn profile_report(
        &self,
        epoch: &str,
        target: &str,
        command: ProfileReportCommand,
    ) -> Result<AccountData<ProfileReportView>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(target))?;
        let owner = token
            .uid
            .as_deref()
            .filter(|owner| {
                uid.len() <= 32 && !uid.starts_with('0') && owner.trim_start_matches('0') != uid
            })
            .ok_or(Error::InvalidInput(
                "report requires an authenticated guest profile",
            ))?;
        let journal = ReportJournal {
            session: &self.session,
            token: &token,
            store: self.session.persistent_store()?,
            kind: "profile-report-receipt-v1",
            lock_prefix: "profile-report",
            key: uid.clone(),
            draft_scope: None,
        };
        let value = self
            .profile_report
            .act(
                &journal,
                command.into(),
                ProfileReportReason::into_wire,
                |reason| {
                    let journal = &journal;
                    let token = &token;
                    let uid = &uid;
                    async move {
                        let _relationship = OperationLock::acquire(
                            journal.store.directory(),
                            owner,
                            &format!("profile-follow:{uid}"),
                        )?;
                        self.session
                            .run_for(token, |api, _| async move {
                                let detail = api
                                    .call(
                                        &format!("/api/w/v1/user/detail/{uid}"),
                                        json!({"all":true,"userId":uid}),
                                        false,
                                        &[200, 403, 404],
                                    )
                                    .await?;
                                if !code_matches(&detail["code"], 200) {
                                    return Ok(ReportStatus::TargetUnavailable);
                                }
                                let profile = &detail["profile"];
                                if identifier(&profile["userId"]).ok().as_deref()
                                    != Some(uid.as_str())
                                {
                                    return Err(Error::Response("report target identity mismatch"));
                                }
                                let status = &profile["accountStatus"];
                                if matches!(
                                    status.as_i64().or_else(|| status.as_str()?.parse().ok()),
                                    Some(30 | 40)
                                ) || js_truthy(&profile["blacklist"])
                                    || js_truthy(&profile["inBlacklist"])
                                {
                                    return Ok(ReportStatus::TargetUnavailable);
                                }
                                journal
                                    .send(
                                        &api,
                                        "/api/report/reportuser",
                                        json!({"userid":uid,"reason":reason}),
                                        true,
                                    )
                                    .await
                            })
                            .await
                    }
                },
            )
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
