use super::{
    EffectAuthorizations, EffectLease, LeaseObservation, LeasePhase, ReleaseRetry, TrialConsent,
    parametric::{current_non_trial, registered_non_trial},
};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{api::EffectsApi, catalog::EffectId, rights::known_non_trial},
};

/// Opaque receipt for a specific native non-trial registration revision.
pub struct NativeEffectReleaseReceipt {
    session: std::sync::Weak<crate::account::AccountSession>,
    token: SessionToken,
    effect: EffectId,
    operation: String,
    revision: u64,
    phase: LeasePhase,
}

impl NativeEffectReleaseReceipt {
    pub fn effect(&self) -> EffectId {
        self.effect
    }

    pub fn phase(&self) -> LeasePhase {
        self.phase
    }
}

impl EffectAuthorizations {
    /// Prepare/download the effect before calling this. The returned result is
    /// a registered interest and current rights, not successful DSP activation.
    /// Release the previous owned registration before selecting a different ID.
    pub async fn begin_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        trial: TrialConsent,
    ) -> Result<LeaseObservation> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.begin_locked(token, &owner, effect, trial, |lease| {
            self.session.with_token(token, |_| self.save(&owner, lease))
        })
        .await
    }

    pub(super) async fn begin_locked(
        &self,
        token: &SessionToken,
        owner: &str,
        effect: EffectId,
        trial: TrialConsent,
        commit: impl FnOnce(&EffectLease) -> Result<()>,
    ) -> Result<LeaseObservation> {
        for lease in self.records_for(token)? {
            if lease.effect != effect && lease.phase.needs_release() {
                return Err(Error::State(
                    "release the previous effect authorization before switching",
                ));
            }
        }
        if let Some(lease) = self.get(owner, effect)? {
            if lease.phase == LeasePhase::Registered {
                if trial == TrialConsent::KnownNonTrial && !registered_non_trial(&lease) {
                    return Err(Error::State(
                        "no acknowledged non-trial effect registration",
                    ));
                }
                return self.observe_begin(token, owner, lease, trial).await;
            }
            if lease.phase.needs_release() {
                return Err(Error::MutationUncertain);
            }
        }
        let authorization = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).authorization(effect).await
            })
            .await?
            .filter(|auth| auth.can_use)
            .ok_or(Error::State("effect registration is not authorized"))?;
        if trial == TrialConsent::KnownNonTrial && !known_non_trial(authorization.can_use_type) {
            return Err(Error::State(
                "effect registration requires confirmed non-trial rights",
            ));
        }
        if authorization.is_trial() && trial != TrialConsent::ExplicitStart {
            return Err(Error::State(
                "starting an effect trial requires an explicit action",
            ));
        }
        let mut lease = EffectLease {
            effect,
            operation: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            phase: LeasePhase::Registering,
            start_acknowledged: false,
            registration_use_type: authorization.can_use_type,
            stop_acknowledged: false,
            authorization: Some(authorization),
            recyclable: None,
            last_error: None,
        };
        commit(&lease)?;
        self.send(token, owner, &mut lease, true).await?;
        self.observe_begin(token, owner, lease, trial).await
    }

    async fn observe_begin(
        &self,
        token: &SessionToken,
        owner: &str,
        lease: EffectLease,
        trial: TrialConsent,
    ) -> Result<LeaseObservation> {
        let mut observation = self.observe_locked(token, owner, lease).await?;
        if trial == TrialConsent::KnownNonTrial && !current_non_trial(&observation.lease) {
            // Preserve the receipt and release path even when the fresh rights
            // observation does not permit this client to use the registration.
            observation.can_use = false;
        }
        Ok(observation)
    }

    /// Read the durable cleanup identity without contacting the service.
    pub fn native_release_receipt_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
    ) -> Result<Option<NativeEffectReleaseReceipt>> {
        Ok(self.record_for(token, effect)?.and_then(|lease| {
            (lease.phase.needs_release() && known_non_trial(lease.registration_use_type)).then(
                || NativeEffectReleaseReceipt {
                    session: std::sync::Arc::downgrade(&self.session),
                    token: token.clone(),
                    effect,
                    operation: lease.operation,
                    revision: lease.revision,
                    phase: lease.phase,
                },
            )
        }))
    }

    /// Explicit stop of the exact observed native registration. All bound
    /// actors, including other processes, must have confirmed local removal.
    pub async fn release_native_for(
        &self,
        token: &SessionToken,
        receipt: &NativeEffectReleaseReceipt,
        retry: ReleaseRetry,
    ) -> Result<EffectLease> {
        if &receipt.token != token
            || !receipt
                .session
                .upgrade()
                .is_some_and(|session| std::sync::Arc::ptr_eq(&self.session, &session))
        {
            return Err(Error::StaleResponse);
        }
        let owner = self.owner(token)?;
        let _activation = self.activation_lock_for(token)?;
        let _unused = self.acquire_bound_use_fence_for(token)?;
        let _lock = self.lock(&owner)?;
        let lease = self
            .get(&owner, receipt.effect)?
            .filter(|lease| {
                lease.operation == receipt.operation
                    && lease.revision == receipt.revision
                    && lease.phase == receipt.phase
                    && known_non_trial(lease.registration_use_type)
            })
            .ok_or(Error::StaleResponse)?;
        let result = self.release_locked(token, &owner, lease, retry).await;
        if result.is_err() {
            self.session.check_generation(token)?;
            if self.get(&owner, receipt.effect)?.is_some_and(|lease| {
                matches!(
                    lease.phase,
                    LeasePhase::Releasing | LeasePhase::StopUncertain
                )
            }) {
                return Err(Error::MutationUncertain);
            }
        }
        result
    }

    pub async fn release_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        retry: ReleaseRetry,
    ) -> Result<EffectLease> {
        let owner = self.owner(token)?;
        let _activation = self.activation_lock_for(token)?;
        let _unused = self.acquire_bound_use_fence_for(token)?;
        let _lock = self.lock(&owner)?;
        let lease = self
            .get(&owner, effect)?
            .ok_or(Error::State("no owned effect authorization to release"))?;
        self.release_locked(token, &owner, lease, retry).await
    }

    async fn release_locked(
        &self,
        token: &SessionToken,
        owner: &str,
        mut lease: EffectLease,
        retry: ReleaseRetry,
    ) -> Result<EffectLease> {
        if lease.phase == LeasePhase::Released {
            return Ok(lease);
        }
        if lease.phase == LeasePhase::StartRejected {
            // No owned registration was accepted. Keep that distinct from a
            // stop acknowledgement rather than manufacturing a Released state.
            return Ok(lease);
        }
        if matches!(
            lease.phase,
            LeasePhase::Releasing | LeasePhase::StopUncertain
        ) && retry != ReleaseRetry::ExplicitAfterUnknownResult
        {
            return Err(Error::MutationUncertain);
        }
        lease.phase = LeasePhase::Releasing;
        lease.last_error = None;
        lease.advance()?;
        self.session
            .with_token(token, |_| self.save(owner, &lease))?;
        self.send(token, owner, &mut lease, false).await?;
        Ok(lease)
    }

    async fn send(
        &self,
        token: &SessionToken,
        owner: &str,
        lease: &mut EffectLease,
        start: bool,
    ) -> Result<()> {
        let result = self
            .session
            .run_for(token, |api, _| async {
                let api = EffectsApi::new(api);
                let result = if start {
                    api.register_interest(lease.effect).await
                } else {
                    api.release_interest(lease.effect).await
                };
                match &result {
                    Ok(()) => {
                        if start {
                            lease.start_acknowledged = true;
                            lease.phase = LeasePhase::Registered;
                        } else {
                            lease.stop_acknowledged = true;
                            lease.phase = LeasePhase::Released;
                        }
                        lease.last_error = None;
                    }
                    Err(error) => {
                        // Explicit business rejection is not an ambiguous transport
                        // failure. Other service codes remain uncertain.
                        let rejected = [301, 400, 403, 405]
                            .iter()
                            .any(|code| error.is_service_code(*code));
                        lease.phase = match (start, rejected) {
                            (true, true) => LeasePhase::StartRejected,
                            (true, false) => LeasePhase::StartUncertain,
                            (false, true) => LeasePhase::StopRejected,
                            (false, false) => LeasePhase::StopUncertain,
                        };
                        lease.last_error = Some(error.to_string());
                    }
                }
                lease.advance()?;
                // Preserve a received acknowledgement in its original account even
                // when the user switched accounts while the request was outstanding.
                self.save(owner, lease)?;
                result
            })
            .await;
        // If the future was cancelled, the durable Sending phase remains. A
        // later lock-owning observe/release handles it without replaying start.
        result
    }
}
