use super::*;
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    protocol::api::{Api, VinylCommentWrite, identifier, validate_response},
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Copy)]
struct VinylLikeRead<'a> {
    api: &'a Arc<dyn Api>,
    revision: &'a str,
}

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
        self.like_for_inner(token, resource, id, desired, None)
            .await
    }

    pub(crate) async fn like_for_vinyl(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        desired: bool,
        authority: VinylWriteAuthority<'_>,
    ) -> Result<CommentLike> {
        self.like_for_inner(token, resource, id, desired, Some(authority))
            .await
    }

    async fn like_for_inner(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        desired: bool,
        vinyl: Option<VinylWriteAuthority<'_>>,
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
        let previous = self.like_record(owner, &key)?;
        if previous.as_ref().is_some_and(|r| r.phase.pending()) {
            return Err(Error::MutationUncertain);
        }
        let vinyl_request = if vinyl.is_some() {
            if resource.kind != ResourceKind::Track || resource.owner.is_some() {
                return Err(Error::InvalidInput("invalid vinyl comment like scope"));
            }
            Some(VinylCommentWrite::like(&resource.id, id, desired)?)
        } else {
            None
        };
        let read_scope = vinyl.as_ref().map(|authority| VinylLikeRead {
            api: authority.api,
            revision: authority.revision,
        });
        let before = self
            .like_target_for(token, resource, id, read_scope)
            .await?;
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
        if vinyl.is_none() || record.phase == MutationPhase::Unchanged {
            self.save_like_transition(token, &key, &record)?;
        }
        if record.phase == MutationPhase::Unchanged {
            return Ok(record);
        }
        let result = if let Some((authority, write)) = vinyl.zip(vinyl_request) {
            let mut persisted = false;
            let mut known_unsent = false;
            let result = self
                .session
                .run_for(token, |api, _| {
                    let (key, record, persisted, known_unsent) =
                        (&key, &record, &mut persisted, &mut known_unsent);
                    async move {
                        if !Arc::ptr_eq(authority.api, &api) {
                            return Err(Error::RequestNotSent(Box::new(Error::StaleResponse)));
                        }
                        // A stale account may fail run_for before this closure;
                        // only mark Sending once the original API is selected.
                        self.save_like_transition(token, key, record)?;
                        *persisted = true;
                        let path = write.path();
                        let response = api
                            .request_vinyl_comment_write(write, authority.revision, authority.permission)
                            .await;
                        // Session rejection may replace this transport error
                        // with StaleResponse after an account switch.
                        *known_unsent = matches!(&response, Err(error) if matches!(error.root(), Error::RequestNotSent(_)));
                        validate_response(path, response?, &[200]).map(|_| ())
                    }
                })
                .await;
            if result.is_err() {
                if !persisted {
                    return result.map(|_| record);
                }
                if known_unsent {
                    // The per-comment lock is still held. Only restore this
                    // unsent intent's original owner, even after account switch.
                    self.store.transaction(|tx| {
                        if let Some(previous) = previous {
                            tx.put(owner, JOURNAL, &key, &serde_json::to_value(previous)?)
                        } else {
                            tx.delete(owner, JOURNAL, &key)
                        }
                    })?;
                    return result.map(|_| record);
                }
            }
            result
        } else {
            self.session
                .run_for(token, |api, _| async move {
                    CommentApi::new(api).like(resource, id, desired).await
                })
                .await
        };
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
        self.settle_like(token, &key, record, read_scope).await
    }
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
    ) -> Result<CommentLike> {
        self.reconcile_like_for(token, resource, id, None).await
    }

    pub(crate) async fn reconcile_for_vinyl(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        api: &Arc<dyn Api>,
        revision: &str,
    ) -> Result<CommentLike> {
        self.reconcile_like_for(token, resource, id, Some(VinylLikeRead { api, revision }))
            .await
    }

    async fn reconcile_like_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        scope: Option<VinylLikeRead<'_>>,
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
        self.settle_like(token, &key, record, scope).await
    }

    async fn like_target_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
        scope: Option<VinylLikeRead<'_>>,
    ) -> Result<Comment> {
        if let Some(scope) = scope {
            self.session
                .run_for(token, |api, _| async move {
                    if !Arc::ptr_eq(scope.api, &api) {
                        return Err(Error::StaleResponse);
                    }
                    let target = CommentApi::new(api)
                        .vinyl_target(resource, id, scope.revision)
                        .await?
                        .ok_or(Error::State("comment is no longer readable"))?;
                    if target.status.is_some_and(|status| status < 0) {
                        return Err(Error::State("comment target is unavailable"));
                    }
                    Ok(target)
                })
                .await
        } else {
            self.target_for(token, resource, id).await
        }
    }
    async fn settle_like(
        &self,
        token: &SessionToken,
        key: &str,
        mut record: CommentLike,
        scope: Option<VinylLikeRead<'_>>,
    ) -> Result<CommentLike> {
        let mut consecutive = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(self.settle).await;
            }
            let observed = self
                .like_target_for(token, &record.resource, &record.comment_id, scope)
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
