//! Explicit effect registration/release with durable uncertain-write handling.
//! Server canUse/canRecycle are entitlement facts, not occupancy receipts.
//! Hosts apply/disable the DSP separately and schedule reads on restore or
//! rights changes. No guessed trial deadline or background write timer.
mod modern;
mod mutate;
pub use modern::NativeModernLimiterBinding;
pub use mutate::{NativeEffectReleaseReceipt, PendingNativeEffectRegistration};
mod observe;
mod parametric;
pub use parametric::{ActiveParametricObservation, BoundParametric, BoundParametricTarget};
mod record;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    effects::catalog::EffectId,
    storage::{OperationLock, StateStore},
};
pub use record::{EffectLease, LeaseObservation, LeasePhase, ReleaseRetry, TrialConsent};
#[derive(Clone, Copy)]
struct RegistrationScope<'a> {
    revision: &'a str,
    current: &'a crate::protocol::api::EffectCataloguePermission<'a>,
}

impl RegistrationScope<'_> {
    async fn authorization(
        scope: Option<Self>,
        api: &crate::effects::api::EffectsApi,
        id: EffectId,
    ) -> Result<Option<crate::effects::rights::Authorization>> {
        match scope {
            Some(scope) => {
                api.guarded_material_authorization(id, scope.revision, scope.current)
                    .await
            }
            None => api.authorization(id).await,
        }
    }
    async fn recyclable(
        scope: Option<Self>,
        api: &crate::effects::api::EffectsApi,
        id: EffectId,
    ) -> Result<Option<bool>> {
        match scope {
            Some(scope) => {
                api.guarded_material_recyclable(id, scope.revision, scope.current)
                    .await
            }
            None => api.recyclable(id).await,
        }
    }
}
use std::sync::Arc;
const JOURNAL: &str = "effect-authorizations-v1";

#[derive(Clone)]
pub struct EffectAuthorizations {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl EffectAuthorizations {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    /// Separate from the lease lock: the playback actor takes that lock
    /// itself while applying the sealed binding.
    pub(crate) fn activation_lock_for(&self, token: &SessionToken) -> Result<OperationLock> {
        let owner = self.owner(token)?;
        OperationLock::acquire(self.store.directory(), "effect-native-activation", &owner)
    }

    pub fn record_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
    ) -> Result<Option<EffectLease>> {
        self.session
            .with_token(token, |owner| self.get(owner, effect))
    }
    pub fn records_for(&self, token: &SessionToken) -> Result<Vec<EffectLease>> {
        self.session.with_token(token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .into_iter()
                .map(|(key, value)| {
                    let lease: EffectLease = serde_json::from_value(value)?;
                    if key != lease.effect.value().to_string() {
                        return Err(Error::State("effect authorization journal key mismatch"));
                    }
                    lease.validate(lease.effect)?;
                    Ok(lease)
                })
                .collect()
        })
    }
    fn get(&self, owner: &str, effect: EffectId) -> Result<Option<EffectLease>> {
        let lease: Option<EffectLease> = self
            .store
            .get(owner, JOURNAL, &effect.value().to_string())?
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(lease) = &lease {
            lease.validate(effect)?;
        }
        Ok(lease)
    }
    fn save(&self, owner: &str, lease: &EffectLease) -> Result<()> {
        lease.validate(lease.effect)?;
        self.store.put(
            owner,
            JOURNAL,
            &lease.effect.value().to_string(),
            &serde_json::to_value(lease)?,
        )
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |owner| Ok(owner.to_owned()))
    }
    fn lock(&self, owner: &str) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), "effect-authorizations", owner)
    }
}
