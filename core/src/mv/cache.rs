use super::{MvDetail, MvLike, MvService, service::DETAILS};
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
};
use serde_json::{Value, json};
const REVISIONS: &str = "mv-detail-revisions-v1";
impl MvService {
    pub(super) fn detail_revision(&self, token: &SessionToken, id: &str) -> Result<Option<Value>> {
        self.session
            .with_token(token, |owner| self.store.get(owner, REVISIONS, id))
    }
    pub(super) fn cache_detail(
        &self,
        token: &SessionToken,
        id: &str,
        revision: Option<Value>,
        detail: MvDetail,
    ) -> Result<MvDetail> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                if tx.get(owner, REVISIONS, id)? != revision {
                    return Err(Error::QueueChanged);
                }
                tx.put(owner, DETAILS, id, &serde_json::to_value(&detail)?)?;
                Ok(detail)
            })
        })
    }
    pub(super) fn save_like_transition(&self, token: &SessionToken, record: &MvLike) -> Result<()> {
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(
                    owner,
                    super::likes::JOURNAL,
                    &record.id,
                    &serde_json::to_value(record)?,
                )?;
                tx.put(
                    owner,
                    REVISIONS,
                    &record.id,
                    &json!(uuid::Uuid::new_v4().simple().to_string()),
                )?;
                if matches!(
                    record.phase,
                    MutationPhase::Confirmed | MutationPhase::Unchanged
                ) {
                    let observed = record
                        .observed
                        .as_ref()
                        .ok_or(Error::State("MV like observation missing"))?;
                    if let Some(value) = tx.get(owner, DETAILS, &record.id)? {
                        let mut detail: MvDetail = serde_json::from_value(value)?;
                        if detail.id != record.id {
                            return Err(Error::State("MV cache identity mismatch"));
                        }
                        detail.liked = observed.liked;
                        detail.liked_known = true;
                        detail.like_count = observed.count;
                        tx.put(owner, DETAILS, &record.id, &serde_json::to_value(detail)?)?;
                    }
                }
                Ok(())
            })
        })
    }
}
