//! The recovered comment card sends the comment thread and ID, not author ID.
use super::{AccountData, CommentResource, MusicClient};
use crate::{
    Error, Result,
    comments::{Comment, CommentApi, ResourceKind},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::Deserialize;
use serde_json::json;

use super::super::{
    profile_report::ProfileReportReason,
    report_receipt::{ReportCommand, ReportJournal, ReportStatus},
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommentReportReason {
    Meaningless,
    Advertising,
    PersonalAttack,
    HarmfulContent,
    Political,
    Other { description: String },
}
impl CommentReportReason {
    fn into_wire(self) -> Result<String> {
        let shared = match self {
            Self::Meaningless => return Ok("段子或者无意义的评论".into()),
            Self::Advertising => ProfileReportReason::Advertising,
            Self::PersonalAttack => ProfileReportReason::PersonalAttack,
            Self::HarmfulContent => ProfileReportReason::HarmfulContent,
            Self::Political => ProfileReportReason::Political,
            Self::Other { description } => ProfileReportReason::Other { description },
        };
        shared.into_wire()
    }
}
pub type CommentReportCommand = ReportCommand<CommentReportReason>;
pub type CommentReportView = super::super::report_receipt::ReportView;

pub(super) fn reportable(comment: &Comment, owner: &str) -> bool {
    comment.status.is_none_or(|status| status >= 0)
        && comment.author.as_ref().is_some_and(|author| {
            !author.unavailable
                && author.id.trim_start_matches('0') != owner.trim_start_matches('0')
        })
}

impl MusicClient {
    pub async fn comment_report(
        &self,
        epoch: &str,
        resource: CommentResource,
        id: &str,
        command: CommentReportCommand,
    ) -> Result<AccountData<CommentReportView>> {
        let token = self.comment_token(epoch)?;
        let thread = resource.thread()?;
        let id = identifier(&json!(id))?;
        if id.len() > 32
            || id.starts_with('0')
            || (resource.kind != ResourceKind::Video && resource.id.starts_with('0'))
            || resource
                .owner
                .as_ref()
                .is_some_and(|owner| owner.len() > 32 || owner.starts_with('0'))
        {
            return Err(Error::InvalidInput("noncanonical comment identifier"));
        }
        let journal = ReportJournal {
            session: &self.session,
            token: &token,
            store: self.session.persistent_store()?,
            kind: "comment-report-receipt-v1",
            lock_prefix: "comment-report",
            key: serde_json::to_string(&(&thread, &id))?,
        };
        let value = self
            .comment_report
            .act(
                &journal,
                command,
                CommentReportReason::into_wire,
                |reason| {
                    let journal = &journal;
                    let token = &token;
                    async move {
                        // Coordinate with comment edit/delete and reply actions that use the same thread lock.
                        let _write = self.session.with_token(token, |owner| {
                            OperationLock::acquire(
                                journal.store.directory(),
                                "comment-write",
                                &serde_json::to_string(&(owner, &thread))?,
                            )
                        })?;
                        self.session
                            .run_for(token, |api, owner| async move {
                                let Some(target) =
                                    CommentApi::new(api.clone()).lookup(&resource, &id).await?
                                else {
                                    return Ok(ReportStatus::TargetUnavailable);
                                };
                                if !reportable(&target, &owner) {
                                    return Ok(ReportStatus::TargetUnavailable);
                                }
                                journal
                                    .send(
                                        &api,
                                        "/api/report/reportcomment",
                                        json!({"threadId":thread,"commentId":id,"reason":reason}),
                                        false,
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
