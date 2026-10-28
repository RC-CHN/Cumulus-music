use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::resources::{AccountResources, MutationPhase, MutationRecord},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeOperationView {
    pub id: String,
    pub revision: String,
    pub desired: bool,
    pub observed: Option<bool>,
    pub phase: MutationPhase,
    pub can_accept: bool,
}
fn operation(record: MutationRecord) -> Result<LikeOperationView> {
    let desired = record
        .desired
        .as_bool()
        .ok_or(Error::State("invalid like intent"))?;
    let observed = record.observed.as_ref().and_then(|v| v.as_bool());
    Ok(LikeOperationView {
        revision: record.revision()?,
        id: record.key,
        desired,
        observed,
        phase: record.phase,
        can_accept: record.phase == MutationPhase::Uncertain
            && record.checked_at.is_some()
            && observed.is_some_and(|v| v != desired),
    })
}
#[derive(Serialize)]
pub struct LikeRecoveryView {
    pub total: usize,
    pub offset: usize,
    pub operations: Vec<LikeOperationView>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LikeRecoveryCommand {
    Reconcile {
        id: String,
        revision: String,
    },
    AcceptCurrent {
        id: String,
        revision: String,
        observed: bool,
    },
}
impl MusicClient {
    /// Local, account-scoped inspection. At most 20 rows cross the UI boundary.
    pub fn like_recovery(
        &self,
        epoch: &str,
        offset: usize,
    ) -> Result<AccountData<LikeRecoveryView>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let records: Vec<_> = AccountResources::new(self.session.clone())?
            .liked_operations_for(&token)?
            .into_iter()
            .filter(|r| r.phase.pending())
            .collect();
        let total = records.len();
        let offset = (offset / 20 * 20).min(total.saturating_sub(1) / 20 * 20);
        let operations = records
            .into_iter()
            .skip(offset)
            .take(20)
            .map(operation)
            .collect::<Result<_>>()?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: LikeRecoveryView {
                    total,
                    offset,
                    operations,
                },
            })
        })
    }
    pub async fn recover_like(
        &self,
        epoch: &str,
        command: LikeRecoveryCommand,
    ) -> Result<AccountData<LikeOperationView>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let (id, revision, observed) = match command {
            LikeRecoveryCommand::Reconcile { id, revision } => (id, revision, None),
            LikeRecoveryCommand::AcceptCurrent {
                id,
                revision,
                observed,
            } => (id, revision, Some(observed)),
        };
        let record = AccountResources::new(self.session.clone())?
            .recover_like_for(&token, &id, &revision, observed)
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: operation(record)?,
            })
        })
    }
}
