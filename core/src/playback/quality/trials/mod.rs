//! A trial start is a quota-consuming write, never an automatic playback retry.
//! Read-only reconciliation can establish current use rights without inventing
//! a lost activation receipt. There is no recovered quality-trial stop API.
pub(crate) mod monitor;
mod mutate;
mod record;
use super::{QualityApi, QualityRight, QualityUseRight};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore, StateTransaction},
};
pub use record::{QualityTrial, QualityTrialPhase, TrialConsent};
use std::{sync::Arc, time::Duration};

const JOURNAL: &str = "quality-trials-v1";
const HISTORY: &str = "quality-trial-history-v1";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct QualityTrials {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl QualityTrials {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |owner| Ok(owner.to_owned()))
    }
    fn lock(&self, owner: &str) -> Result<OperationLock> {
        // Serialize all four starts and observations for one account, including
        // another process. No account/SQLite lock spans a network await.
        OperationLock::acquire(self.store.directory(), "quality-trials", owner)
    }
    fn get(&self, owner: &str, right: QualityRight) -> Result<Option<QualityTrial>> {
        self.store.transaction(|tx| Self::get_in(tx, owner, right))
    }
    fn get_in(
        tx: &mut StateTransaction<'_>,
        owner: &str,
        right: QualityRight,
    ) -> Result<Option<QualityTrial>> {
        let value: Option<QualityTrial> = tx
            .get(owner, JOURNAL, right.quality().as_str())?
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(record) = &value {
            record.validate(right)?;
        }
        Ok(value)
    }
    fn save(&self, owner: &str, record: &QualityTrial) -> Result<()> {
        record.validate(record.right)?;
        self.store.transaction(|tx| {
            let existing = Self::get_in(tx, owner, record.right)?;
            if let Some(existing) = existing {
                if existing.operation != record.operation {
                    if !existing.phase.terminal() {
                        return Err(Error::MutationUncertain);
                    }
                    tx.put(
                        owner,
                        HISTORY,
                        &existing.operation,
                        &serde_json::to_value(&existing)?,
                    )?;
                } else if existing.revision >= record.revision {
                    return Err(Error::State("quality trial journal changed"));
                }
            }
            tx.put(
                owner,
                JOURNAL,
                record.right.quality().as_str(),
                &serde_json::to_value(record)?,
            )
        })
    }
    pub fn record_for(
        &self,
        token: &SessionToken,
        right: QualityRight,
    ) -> Result<Option<QualityTrial>> {
        self.session
            .with_token(token, |owner| self.get(owner, right))
    }
    pub fn records_for(&self, token: &SessionToken) -> Result<Vec<QualityTrial>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_iter()
                .map(|(key, value)| {
                    let record: QualityTrial = serde_json::from_value(value)?;
                    record.validate(record.right)?;
                    if key != record.right.quality().as_str() {
                        return Err(Error::State("quality trial journal key mismatch"));
                    }
                    Ok(record)
                })
                .collect()
        })
    }
    pub(crate) fn require_active_in(
        tx: &mut StateTransaction<'_>,
        owner: &str,
        right: QualityRight,
        operation: &str,
    ) -> Result<()> {
        let record =
            Self::get_in(tx, owner, right)?.ok_or(Error::State("quality trial is not active"))?;
        if record.operation != operation || !record.can_use() {
            return Err(Error::State("quality trial is not active"));
        }
        Ok(())
    }
    pub(crate) fn require_expired_in(
        tx: &mut StateTransaction<'_>,
        owner: &str,
        right: QualityRight,
        operation: &str,
    ) -> Result<()> {
        let record =
            Self::get_in(tx, owner, right)?.ok_or(Error::State("quality trial is not expired"))?;
        if record.operation != operation
            || record.phase != QualityTrialPhase::Expired
            || record.read_failed
            || record.observed.as_ref().is_none_or(|r| r.can_use)
        {
            return Err(Error::State("quality trial is not expired"));
        }
        Ok(())
    }
    async fn query(
        &self,
        token: &SessionToken,
        right: QualityRight,
    ) -> Result<Option<QualityUseRight>> {
        tokio::time::timeout(
            TIMEOUT,
            self.session.run_for(token, |api, _| async move {
                QualityApi::new(api).use_right(right).await
            }),
        )
        .await
        .map_err(|_| Error::State("quality trial read timed out"))?
    }
}
