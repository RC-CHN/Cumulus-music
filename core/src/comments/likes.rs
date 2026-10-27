use super::*;
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
pub(super) const JOURNAL: &str = "comment-likes-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct CommentLike {
    pub resource: CommentResource,
    pub comment_id: String,
    pub desired: bool,
    pub before: bool,
    pub acknowledged: bool,
    pub phase: MutationPhase,
    pub observed: Option<Comment>,
}
fn key(resource: &CommentResource, id: &str) -> Result<String> {
    Ok(format!(
        "{}:{}",
        resource.thread()?,
        identifier(&json!(id))?
    ))
}
impl Comments {
    fn like_record(&self, owner: &str, key: &str) -> Result<Option<CommentLike>> {
        let record: Option<CommentLike> = self
            .store
            .get(owner, JOURNAL, key)?
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(r) = &record
            && self::key(&r.resource, &r.comment_id)? != key
        {
            return Err(Error::State("comment like journal identity mismatch"));
        }
        Ok(record)
    }
    pub fn operations_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
    ) -> Result<Vec<CommentLike>> {
        resource.thread()?;
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_keys()
                .filter_map(|k| match self.like_record(owner, &k) {
                    Ok(Some(r)) if &r.resource == resource && r.phase.pending() => Some(Ok(r)),
                    Ok(_) => None,
                    Err(e) => Some(Err(e)),
                })
                .collect()
        })
    }
    pub async fn like_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        desired: bool,
    ) -> Result<CommentLike> {
        let key = key(resource, id)?;
        self.session.with_token(token, |_| Ok(()))?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing comment account"))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            "comment-like",
            &serde_json::to_string(&(owner, &key))?,
        )?;
        if self
            .like_record(owner, &key)?
            .is_some_and(|r| r.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let before = self.target_for(token, resource, id).await?;
        let before_liked = before
            .liked
            .ok_or(Error::Response("comment liked flag is unavailable"))?;
        let mut record = CommentLike {
            resource: resource.clone(),
            comment_id: id.into(),
            desired,
            before: before_liked,
            acknowledged: false,
            phase: if before_liked == desired {
                MutationPhase::Unchanged
            } else {
                MutationPhase::Sending
            },
            observed: Some(before),
        };
        self.save_like_transition(token, &key, &record)?;
        if record.phase == MutationPhase::Unchanged {
            return Ok(record);
        }
        let result = self
            .session
            .run_for(token, |api, _| async move {
                CommentApi::new(api).like(resource, id, desired).await
            })
            .await;
        match result {
            Ok(()) => {
                record.phase = MutationPhase::Verifying;
                record.acknowledged = true;
            }
            Err(error) => {
                record.phase = if [301, 403, 405, 505, 506, 511, 512]
                    .iter()
                    .any(|code| error.is_service_code(*code))
                {
                    MutationPhase::Rejected
                } else {
                    MutationPhase::Uncertain
                };
                // This may be a late response after an account switch. Keep the
                // write responsibility under its original owner, never the new one.
                self.store
                    .put(owner, JOURNAL, &key, &serde_json::to_value(&record)?)?;
                return Err(error);
            }
        }
        self.store
            .put(owner, JOURNAL, &key, &serde_json::to_value(&record)?)?;
        self.settle_like(token, &key, record).await
    }
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
    ) -> Result<CommentLike> {
        let key = key(resource, id)?;
        self.session.with_token(token, |_| Ok(()))?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing comment account"))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            "comment-like",
            &serde_json::to_string(&(owner, &key))?,
        )?;
        let record = self
            .like_record(owner, &key)?
            .ok_or(Error::State("no comment operation to reconcile"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.settle_like(token, &key, record).await
    }
    async fn settle_like(
        &self,
        token: &SessionToken,
        key: &str,
        mut record: CommentLike,
    ) -> Result<CommentLike> {
        let mut consecutive = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(self.settle).await;
            }
            let observed = self
                .target_for(token, &record.resource, &record.comment_id)
                .await?;
            consecutive = if observed.liked == Some(record.desired) {
                consecutive + 1
            } else {
                0
            };
            record.observed = Some(observed);
            record.phase = if consecutive >= 2 {
                MutationPhase::Confirmed
            } else {
                MutationPhase::Uncertain
            };
            if record.phase == MutationPhase::Confirmed {
                self.save_like_transition(token, key, &record)?;
            } else {
                self.session
                    .save_result(token, JOURNAL, key, &serde_json::to_value(&record)?)?;
            }
            if record.phase == MutationPhase::Confirmed {
                return Ok(record);
            }
        }
        Err(Error::MutationUncertain)
    }
}
