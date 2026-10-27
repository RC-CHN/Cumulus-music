use super::{MvApi, MvService};
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
pub(super) const JOURNAL: &str = "mv-likes-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct MvLikeState {
    pub liked: bool,
    pub count: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MvLike {
    pub id: String,
    pub before: bool,
    pub desired: bool,
    pub acknowledged: bool,
    pub phase: MutationPhase,
    pub observed: Option<MvLikeState>,
}
impl MvService {
    pub fn like_operation_for(&self, token: &SessionToken, id: &str) -> Result<Option<MvLike>> {
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |owner| {
            let record: Option<MvLike> = self
                .store
                .get(owner, JOURNAL, &id)?
                .map(serde_json::from_value)
                .transpose()?;
            if record.as_ref().is_some_and(|r| r.id != id) {
                return Err(Error::State("MV like journal identity mismatch"));
            }
            Ok(record)
        })
    }
    pub async fn like_state_for(&self, token: &SessionToken, id: &str) -> Result<MvLikeState> {
        let detail = self.detail_for(token, &json!(id)).await?;
        if !detail.liked_known || detail.thread_id != format!("R_MV_5_{}", detail.id) {
            return Err(Error::Response(
                "MV like state or thread identity is unavailable",
            ));
        }
        Ok(MvLikeState {
            liked: detail.liked,
            count: detail.like_count,
        })
    }
    pub async fn like_for(&self, token: &SessionToken, id: &str, desired: bool) -> Result<MvLike> {
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |_| Ok(()))?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing MV account"))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            "mv-like",
            &serde_json::to_string(&(owner, &id))?,
        )?;
        if self
            .like_operation_for(token, &id)?
            .is_some_and(|r| r.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let observed = self.like_state_for(token, &id).await?;
        let mut record = MvLike {
            id: id.clone(),
            before: observed.liked,
            desired,
            acknowledged: false,
            phase: if observed.liked == desired {
                MutationPhase::Unchanged
            } else {
                MutationPhase::Sending
            },
            observed: Some(observed),
        };
        self.save_like_transition(token, &record)?;
        if record.phase == MutationPhase::Unchanged {
            return Ok(record);
        }
        let result = self
            .session
            .run_for(token, |api, _| async {
                MvApi::new(api).like(&json!(id), desired).await
            })
            .await;
        match result {
            Ok(()) => {
                record.acknowledged = true;
                record.phase = MutationPhase::Verifying;
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
                self.store
                    .put(owner, JOURNAL, &id, &serde_json::to_value(&record)?)?;
                return Err(error);
            }
        }
        self.store
            .put(owner, JOURNAL, &id, &serde_json::to_value(&record)?)?;
        self.settle_like(token, record).await
    }
    pub async fn reconcile_like_for(&self, token: &SessionToken, id: &str) -> Result<MvLike> {
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |_| Ok(()))?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing MV account"))?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            "mv-like",
            &serde_json::to_string(&(owner, &id))?,
        )?;
        let record = self
            .like_operation_for(token, &id)?
            .ok_or(Error::State("no MV like operation to reconcile"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        self.settle_like(token, record).await
    }
    async fn settle_like(&self, token: &SessionToken, mut record: MvLike) -> Result<MvLike> {
        let mut consecutive = 0;
        for attempt in 0..8 {
            if attempt > 0 {
                tokio::time::sleep(self.settle_delay).await;
            }
            let observed = self.like_state_for(token, &record.id).await?;
            consecutive = if observed.liked == record.desired {
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
                self.save_like_transition(token, &record)?;
                return Ok(record);
            }
            self.session.save_result(
                token,
                JOURNAL,
                &record.id,
                &serde_json::to_value(&record)?,
            )?;
        }
        Err(Error::MutationUncertain)
    }
}
