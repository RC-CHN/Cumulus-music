//! Bind verified material to an already acknowledged registration. This does
//! not register, release, start a trial, or hand a plan to the audio engine.
use super::{EffectAuthorizations, EffectLease, LeasePhase};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{
        catalog::EffectId,
        service::{EffectsLibrary, PreparedParametric},
    },
};
use std::sync::Arc;

/// Native-only proof of a particular registration observation and prepared
/// material. It is not serializable, a DSP receipt, or a perpetual permission.
pub struct BoundParametric {
    prepared: Box<PreparedParametric>,
    operation: String,
    revision: u64,
}

impl BoundParametric {
    pub fn effect(&self) -> EffectId {
        self.prepared.effect()
    }
}

fn known_non_trial(value: Option<u32>) -> bool {
    // Confirmed non-trial kinds from the existing guarded activation proof.
    matches!(value, Some(10..=12))
}

fn registered_non_trial(lease: &EffectLease) -> bool {
    lease.phase == LeasePhase::Registered
        && lease.start_acknowledged
        && !lease.stop_acknowledged
        && known_non_trial(lease.registration_use_type)
}

fn current_non_trial(lease: &EffectLease) -> bool {
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
        if !prepared.enabled() || !prepared.additional_sections().is_empty() {
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
            prepared,
            operation: observation.lease.operation,
            revision: observation.lease.revision,
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
        if !Arc::ptr_eq(&self.session, &library.session) {
            return Err(Error::StaleResponse);
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let lease = self.get(&owner, binding.effect())?;
        if !lease.is_some_and(|lease| {
            lease.operation == binding.operation
                && lease.revision == binding.revision
                && current_non_trial(&lease)
        }) {
            return Err(Error::StaleResponse);
        }
        library.prepared_parametric_for(token, &binding.prepared)?;
        Ok(())
    }
}
