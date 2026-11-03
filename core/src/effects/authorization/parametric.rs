//! Bind verified material to an already acknowledged registration. This does
//! not register, release, start a trial, or hand a plan to the audio engine.
use super::{EffectAuthorizations, EffectLease, LeasePhase};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{
        api::EffectsApi,
        catalog::EffectId,
        rights::known_non_trial,
        service::{EffectsLibrary, PreparedParametric},
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveParametricObservation {
    NoActiveBinding,
    Confirmed,
    Revoked,
}

const OBSERVATION_BUDGET: Duration = Duration::from_secs(5);

/// Native-only proof of a particular registration observation and prepared
/// material. It is not serializable, a DSP receipt, or a perpetual permission.
pub struct BoundParametric {
    target: BoundParametricTarget,
    prepared: Box<PreparedParametric>,
    library: EffectsLibrary,
    operation: String,
    revision: u64,
    observing: AtomicBool,
    revoked: AtomicBool,
    retired: AtomicBool,
}

/// Opaque identity for conditionally removing this exact local binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundParametricTarget(uuid::Uuid);

impl BoundParametric {
    pub fn target(&self) -> BoundParametricTarget {
        self.target
    }

    pub fn effect(&self) -> EffectId {
        self.prepared.effect()
    }

    pub(crate) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    pub(crate) async fn observe_active(
        &self,
        token: &SessionToken,
    ) -> Result<ActiveParametricObservation> {
        if self.observing.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.observing);
        let session = &self.library.session;
        self.with_current(session, token, |_| Ok(()))?;
        let effect = self.effect();
        let facts = tokio::time::timeout(OBSERVATION_BUDGET, async {
            let authorization = session
                .run_for(token, |api, _| async move {
                    EffectsApi::new(api).authorization(effect).await
                })
                .await?;
            let recyclable = session
                .run_for(token, |api, _| async move {
                    EffectsApi::new(api).recyclable(effect).await
                })
                .await?;
            Ok::<_, Error>((authorization, recyclable))
        })
        .await
        .unwrap_or(Err(Error::State("effect rights observation timed out")));
        if facts.as_ref().is_ok_and(|(authorization, recyclable)| {
            *recyclable == Some(false)
                && authorization
                    .as_ref()
                    .is_some_and(|auth| auth.can_use && known_non_trial(auth.can_use_type))
        }) {
            // No durable revision change and no DSP reset on a positive
            // observation. Recheck that neither owner nor lease changed.
            self.with_current(session, token, |_| Ok(()))?;
            return Ok(ActiveParametricObservation::Confirmed);
        }

        if self.retired.load(Ordering::Acquire) {
            return Err(Error::StaleResponse);
        }

        // The active controller must fail closed even if a concurrent journal
        // operation prevents persisting this exact observation.
        self.revoked.store(true, Ordering::Release);
        let authorizations = EffectAuthorizations::new(session.clone())?;
        let owner = authorizations.owner(token)?;
        let _lock = authorizations.lock(&owner)?;
        let mut lease = authorizations.matching_lease(&owner, self)?;
        match &facts {
            Ok((authorization, recyclable)) => {
                lease.authorization = authorization.clone();
                lease.recyclable = *recyclable;
                lease.last_error = None;
            }
            Err(_) => {
                lease.authorization = None;
                lease.recyclable = None;
                lease.last_error = Some("active effect rights observation failed".into());
            }
        }
        lease.advance()?;
        session.with_token(token, |_| authorizations.save(&owner, &lease))?;
        facts.map(|_| ActiveParametricObservation::Revoked)
    }

    pub(crate) fn with_current<T>(
        &self,
        session: &Arc<crate::account::AccountSession>,
        token: &SessionToken,
        action: impl FnOnce(crate::effects::parametric::ParametricPlan) -> Result<T>,
    ) -> Result<T> {
        EffectAuthorizations::new(session.clone())?.with_validated(
            &self.library,
            token,
            self,
            action,
        )
    }
}

pub(super) fn registered_non_trial(lease: &EffectLease) -> bool {
    lease.phase == LeasePhase::Registered
        && lease.start_acknowledged
        && !lease.stop_acknowledged
        && known_non_trial(lease.registration_use_type)
}

pub(super) fn current_non_trial(lease: &EffectLease) -> bool {
    registered_non_trial(lease)
        && lease.last_error.is_none()
        && lease.recyclable == Some(false)
        && lease
            .authorization
            .as_ref()
            .is_some_and(|auth| auth.can_use && known_non_trial(auth.can_use_type))
}

impl EffectAuthorizations {
    /// Read fresh rights under the owner authorization lock, then bind one
    /// enabled PEQ-only material to the exact acknowledged observation.
    /// Additional routed sections, including off sections, are not treated as
    /// a fully implemented graph. All requests here have read semantics.
    pub async fn bind_parametric_for(
        &self,
        library: &EffectsLibrary,
        token: &SessionToken,
        prepared: Box<PreparedParametric>,
    ) -> Result<BoundParametric> {
        if !Arc::ptr_eq(&self.session, &library.session) {
            return Err(Error::StaleResponse);
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        library.prepared_parametric_for(token, &prepared)?;
        if !prepared.is_standalone() {
            return Err(Error::State(
                "effect is not a standalone enabled PEQ material",
            ));
        }
        let lease = self
            .get(&owner, prepared.effect())?
            .filter(registered_non_trial)
            .ok_or(Error::State(
                "no acknowledged non-trial effect registration",
            ))?;
        let observation = self.observe_locked(token, &owner, lease).await?;
        if !observation.can_use || !current_non_trial(&observation.lease) {
            return Err(Error::State("effect activation authority is unavailable"));
        }
        // Acquisition/removal may run during the rights lookup. Recheck the
        // immutable source receipt before publishing a native binding.
        library.prepared_parametric_for(token, &prepared)?;
        Ok(BoundParametric {
            target: BoundParametricTarget(uuid::Uuid::new_v4()),
            prepared,
            library: library.clone(),
            operation: observation.lease.operation,
            revision: observation.lease.revision,
            observing: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
            retired: AtomicBool::new(false),
        })
    }

    /// Read-only local fence. Release, failed or new observations, removal,
    /// owner change and session replacement invalidate an old binding. A
    /// successful check is only valid at the check boundary: a future native
    /// playback integration must continue to enforce revocation.
    pub fn validate_bound_parametric_for(
        &self,
        library: &EffectsLibrary,
        token: &SessionToken,
        binding: &BoundParametric,
    ) -> Result<()> {
        self.with_validated(library, token, binding, |_| Ok(()))
    }

    fn with_validated<T>(
        &self,
        library: &EffectsLibrary,
        token: &SessionToken,
        binding: &BoundParametric,
        action: impl FnOnce(crate::effects::parametric::ParametricPlan) -> Result<T>,
    ) -> Result<T> {
        if !Arc::ptr_eq(&self.session, &library.session) {
            return Err(Error::StaleResponse);
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        if binding.revoked.load(Ordering::Acquire) || binding.retired.load(Ordering::Acquire) {
            return Err(Error::StaleResponse);
        }
        self.matching_lease(&owner, binding)?;
        library.with_prepared_parametric_for(token, &binding.prepared, action)
    }

    fn matching_lease(&self, owner: &str, binding: &BoundParametric) -> Result<EffectLease> {
        self.get(owner, binding.effect())?
            .filter(|lease| {
                lease.operation == binding.operation
                    && lease.revision == binding.revision
                    && current_non_trial(lease)
            })
            .ok_or(Error::StaleResponse)
    }
}
