//! A fresh acknowledged registration bound to one owned WAV and material.
//! No graph, playback handoff, or UI entry is authorized by this module.
use super::{
    EffectAuthorizations, LeasePhase, RegistrationScope, TrialConsent,
    parametric::current_non_trial,
};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{
        catalog::EffectId,
        service::{EffectsLibrary, VerifiedModernLimiterMaterial},
    },
    playback::controller::ModernEffectSource,
};
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// Opaque native pre-handoff binding. There is no serialized state or
/// operation that can start playback through this receipt.
pub struct NativeModernLimiterBinding {
    material: Box<VerifiedModernLimiterMaterial>,
    source: ModernEffectSource,
    library: EffectsLibrary,
    operation: String,
    revision: u64,
}

impl NativeModernLimiterBinding {
    pub fn effect(&self) -> EffectId {
        self.material.effect()
    }
}

impl EffectAuthorizations {
    pub(crate) async fn register_owned_modern_limiter_for(
        &self,
        library: &EffectsLibrary,
        token: &SessionToken,
        material: Box<VerifiedModernLimiterMaterial>,
        source: ModernEffectSource,
    ) -> Result<NativeModernLimiterBinding> {
        if !Arc::ptr_eq(&self.session, &library.session) {
            return Err(Error::StaleResponse);
        }
        source.check_for(&self.session, token)?;
        material.require_live_format(source.format())?;
        let owner = self.owner(token)?;
        let _activation = self.activation_lock_for(token)?;
        let _unused = self.acquire_bound_use_fence_for(token)?;
        let _journal = self.lock(&owner)?;
        let _material = library
            .hold_modern_limiter_material(token, &material)
            .await?;
        let effect = material.effect();
        let current = || {
            source.check_for(&self.session, token)?;
            library.check_modern_limiter_material(token, &material)?;
            source.check_for(&self.session, token)
        };
        current()?;
        let observation = self
            .begin_locked(
                token,
                &owner,
                effect,
                TrialConsent::KnownNonTrial,
                Some(RegistrationScope {
                    revision: material.revision(),
                    current: &current,
                }),
                |lease| {
                    current()?;
                    self.session.with_token(token, |_| self.save(&owner, lease))
                },
            )
            .await;
        let observation = match observation {
            Ok(observation) => observation,
            Err(error) => {
                self.session.check_generation(token)?;
                if self.get(&owner, effect)?.is_some_and(|lease| {
                    matches!(
                        lease.phase,
                        LeasePhase::Registering | LeasePhase::StartUncertain
                    )
                }) {
                    return Err(Error::MutationUncertain);
                }
                return Err(error);
            }
        };
        if !observation.can_use || !current_non_trial(&observation.lease) {
            return Err(Error::State(
                "modern effect activation authority is unavailable",
            ));
        }
        // A successful acknowledgement is preserved in the journal even if
        // the original source, credentials or material fail the handoff gate.
        library
            .verify_locked_modern_limiter_material(token, &material)
            .await?;
        current()?;
        Ok(NativeModernLimiterBinding {
            material,
            source,
            library: library.clone(),
            operation: observation.lease.operation,
            revision: observation.lease.revision,
        })
    }

    /// Verify the exact original binding locally. The durable registration
    /// revision is checked under lock and is not refreshed by this call.
    pub(crate) async fn validate_owned_modern_limiter_for(
        &self,
        token: &SessionToken,
        binding: &NativeModernLimiterBinding,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.session, &binding.library.session) {
            return Err(Error::StaleResponse);
        }
        binding.source.check_for(&self.session, token)?;
        let owner = self.owner(token)?;
        let _journal = self.lock(&owner)?;
        self.get(&owner, binding.effect())?
            .filter(|lease| {
                lease.operation == binding.operation
                    && lease.revision == binding.revision
                    && current_non_trial(lease)
            })
            .ok_or(Error::StaleResponse)?;
        binding
            .library
            .validate_modern_limiter_preflight_for(token, &binding.material)
            .await?;
        binding.source.check_for(&self.session, token)
    }
}
