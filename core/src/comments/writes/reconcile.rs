use super::*;
use crate::{
    Error, Result,
    account::SessionToken,
    comments::{Comment, Comments},
};
impl Comments {
    pub async fn reconcile_write_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<CommentWrite> {
        let record = self.write_operation_for(token, id)?;
        let _lock = self.write_lock(token, &record.resource)?;
        let record = self.write_operation_for(token, id)?;
        self.settle_write(token, record).await
    }
    pub(super) async fn settle_write(
        &self,
        token: &SessionToken,
        mut record: CommentWrite,
    ) -> Result<CommentWrite> {
        if !record.phase.pending() || record.phase == CommentWritePhase::Prepared {
            return Ok(record);
        }
        let id = match &record.command {
            CommentWriteCommand::Delete { comment_id } => comment_id.clone(),
            CommentWriteCommand::Publish { .. } => match &record.receipt_id {
                Some(id) => id.clone(),
                None => {
                    record.phase = CommentWritePhase::Uncertain;
                    self.save_write_transition(token, &record)?;
                    return Err(Error::MutationUncertain);
                }
            },
        };
        let mut consecutive = 0;
        for attempt in 0..4 {
            if attempt > 0 {
                tokio::time::sleep(self.settle).await;
            }
            let observed = self.lookup_for(token, &record.resource, &id).await?;
            let matches = match &record.command {
                CommentWriteCommand::Delete { .. } => {
                    record.acknowledged
                        && observed
                            .as_ref()
                            .is_none_or(|c| c.status.is_some_and(|s| s < 0))
                }
                CommentWriteCommand::Publish { content, reply_to } => {
                    observed.as_ref().is_some_and(|c| {
                        let expected = record
                            .receipt
                            .as_ref()
                            .filter(|r| {
                                r.author
                                    .as_ref()
                                    .is_some_and(|a| Some(a.id.as_str()) == token.uid.as_deref())
                            })
                            .map_or(content.as_str(), |r| r.content.as_str());
                        visible(c)
                            && c.author
                                .as_ref()
                                .is_some_and(|a| Some(a.id.as_str()) == token.uid.as_deref())
                            && c.content == expected
                            && reply_to.as_ref().is_none_or(|id| {
                                c.replies.iter().any(|r| r.id.as_ref() == Some(id))
                            })
                    })
                }
            };
            consecutive = if matches { consecutive + 1 } else { 0 };
            record.observed_absent = observed.is_none();
            record.observed = observed;
            record.phase = if consecutive >= 2 {
                CommentWritePhase::Confirmed
            } else {
                CommentWritePhase::Uncertain
            };
            self.save_write_transition(token, &record)?;
            if record.phase == CommentWritePhase::Confirmed {
                return Ok(record);
            }
        }
        Err(Error::MutationUncertain)
    }
}
fn visible(c: &Comment) -> bool {
    c.status.is_none_or(|s| s >= 0) && !c.content.is_empty()
}
