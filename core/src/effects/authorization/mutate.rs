use super::{
    EffectAuthorizations, EffectLease, LeaseObservation, LeasePhase, ReleaseRetry, TrialConsent,
};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{api::EffectsApi, catalog::EffectId},
};

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
        for lease in self.records_for(token)? {
            if lease.effect != effect && lease.phase.needs_release() {
                return Err(Error::State(
                    "release the previous effect authorization before switching",
                ));
            }
        }
        if let Some(lease) = self.get(&owner, effect)? {
            if lease.phase == LeasePhase::Registered {
                return self.observe_locked(token, &owner, lease).await;
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
            stop_acknowledged: false,
            authorization: Some(authorization),
            recyclable: None,
            last_error: None,
        };
        self.session
            .with_token(token, |_| self.save(&owner, &lease))?;
        self.send(token, &owner, &mut lease, true).await?;
        self.observe_locked(token, &owner, lease).await
    }

    pub async fn release_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        retry: ReleaseRetry,
    ) -> Result<EffectLease> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut lease = self
            .get(&owner, effect)?
            .ok_or(Error::State("no owned effect authorization to release"))?;
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
            .with_token(token, |_| self.save(&owner, &lease))?;
        self.send(token, &owner, &mut lease, false).await?;
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
