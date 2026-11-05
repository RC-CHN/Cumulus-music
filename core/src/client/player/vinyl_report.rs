//! Current native play binds a moderation draft and its actual send.
use super::MusicClient;
use crate::{
    Error, Result,
    client::{
        comments::{CommentReportCommand, CommentReportReason, CommentReportView, reportable},
        dto::AccountData,
        report_receipt::ReportStatus,
    },
    comments::{CommentApi, VinylWriteAuthority},
    protocol::api::{VinylCommentRead, VinylCommentWrite},
    storage::OperationLock,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

impl MusicClient {
    pub async fn player_vinyl_comment_report(
        &self,
        epoch: &str,
        play_id: &str,
        id: &str,
        command: CommentReportCommand,
    ) -> Result<AccountData<CommentReportView>> {
        let target = self.vinyl_write_target(epoch, play_id).await?;
        VinylCommentRead::target(&target.resource.id, id)?;
        let thread = target.resource.thread()?;
        let draft_scope =
            serde_json::to_string(&("vinyl", &thread, id, play_id, &target.revision))?;
        let journal = self.comment_report_journal(&target.token, &thread, id, Some(draft_scope))?;
        let value = self
            .comment_report
            .act(
                &journal,
                command,
                CommentReportReason::into_wire,
                |reason| {
                    let (journal, target, thread) = (&journal, &target, &thread);
                    async move {
                        let _write = self.session.with_token(&target.token, |owner| {
                            OperationLock::acquire(
                                journal.store.directory(),
                                "comment-write",
                                &serde_json::to_string(&(owner, thread))?,
                            )
                        })?;
                        self.session
                            .run_for(&target.token, |api, owner| async move {
                                if !Arc::ptr_eq(&api, &target.api) {
                                    return Err(Error::StaleResponse);
                                }
                                let comment = CommentApi::new(api)
                                    .vinyl_target(&target.resource, id, &target.revision)
                                    .await?;
                                if !comment.as_ref().is_some_and(|row| reportable(row, &owner)) {
                                    return Ok(ReportStatus::TargetUnavailable);
                                }
                                let used = AtomicBool::new(false);
                                let permission = || {
                                    target.permit_live(self, play_id, || {
                                        if used.swap(true, Ordering::Relaxed) {
                                            return Err(Error::StaleResponse);
                                        }
                                        Ok(())
                                    })
                                };
                                journal
                                    .send_vinyl(
                                        VinylCommentWrite::report(&target.resource.id, id, reason)?,
                                        VinylWriteAuthority {
                                            api: &target.api,
                                            revision: &target.revision,
                                            permission: &permission,
                                        },
                                    )
                                    .await
                            })
                            .await
                    }
                },
            )
            .await?;
        target.check(self, play_id).await?;
        self.session.with_token(&target.token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
