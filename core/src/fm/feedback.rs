//! Skip has no recovered read-back endpoint. Once sending starts, an unknown
//! result must remain unknown; re-opening a process never sends it again.
use super::*;
use crate::{
    Error, Result, account::SessionToken, protocol::api::identifier, storage::OperationLock,
};
use serde_json::json;

pub(super) const SKIPS: &str = "fm-skip-v1";
pub(super) fn rejected(error: &Error) -> bool {
    [301, 403, 405, 505, 506, 511, 512]
        .into_iter()
        .any(|code| error.is_service_code(code))
}
impl FmService {
    fn skip_record(&self, owner: &str, id: &str) -> Result<Option<SkipReport>> {
        self.store
            .get(owner, SKIPS, id)?
            .map(|value| {
                let r: SkipReport = serde_json::from_value(value)?;
                if r.id != id || uuid::Uuid::parse_str(id).is_err() || r.seconds > u32::MAX as u64 {
                    return Err(Error::State("invalid FM skip journal identity"));
                }
                identifier(&json!(r.song_id))?;
                Ok(r)
            })
            .transpose()
    }
    pub fn skip_operation_for(&self, token: &SessionToken, id: &str) -> Result<Option<SkipReport>> {
        self.session
            .with_token(token, |owner| self.skip_record(owner, id))
    }
    pub fn skip_operations_for(&self, token: &SessionToken) -> Result<Vec<SkipReport>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, SKIPS)?
                .into_keys()
                .map(|id| {
                    self.skip_record(owner, &id)?
                        .ok_or(Error::State("FM skip journal disappeared"))
                })
                .collect()
        })
    }
    pub async fn dispatch_skip_for(&self, token: &SessionToken, id: &str) -> Result<SkipReport> {
        let owner = self.owner(token)?;
        let _gate =
            OperationLock::acquire(self.store.directory(), SKIPS, &format!("{owner}:{id}"))?;
        let mut record = self
            .skip_operation_for(token, id)?
            .ok_or(Error::State("FM skip missing"))?;
        match record.phase {
            SkipPhase::Prepared => record.phase = SkipPhase::Sending,
            SkipPhase::Sending => {
                record.phase = SkipPhase::Uncertain;
                self.session.with_token(token, |uid| {
                    self.store
                        .put(uid, SKIPS, id, &serde_json::to_value(&record)?)
                })?;
                return Ok(record);
            }
            _ => return Ok(record),
        }
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, SKIPS, id, &serde_json::to_value(&record)?)
        })?;
        let result = self
            .session
            .run_for(token, |api, _| async {
                FmApi::new(api).skip(&record).await
            })
            .await;
        record.phase = match &result {
            Ok(()) => SkipPhase::Confirmed,
            Err(e) if rejected(e) => SkipPhase::Rejected,
            Err(_) => SkipPhase::Uncertain,
        };
        self.persist_original(&owner, SKIPS, id, serde_json::to_value(&record)?)?;
        result?;
        self.owner(token)?;
        Ok(record)
    }
    /// Only unsent feedback may be abandoned. There is no way to turn an
    /// uncertain report into a confirmed server result or a safe retry.
    pub fn abandon_skip_for(&self, token: &SessionToken, id: &str) -> Result<SkipReport> {
        let owner = self.owner(token)?;
        let _gate =
            OperationLock::acquire(self.store.directory(), SKIPS, &format!("{owner}:{id}"))?;
        let mut record = self
            .skip_operation_for(token, id)?
            .ok_or(Error::State("FM skip missing"))?;
        if record.phase != SkipPhase::Prepared {
            return Err(Error::State("only prepared FM feedback can be abandoned"));
        }
        record.phase = SkipPhase::Abandoned;
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, SKIPS, id, &serde_json::to_value(&record)?)
        })?;
        Ok(record)
    }
}
