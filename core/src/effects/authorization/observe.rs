use super::{EffectAuthorizations, EffectLease, LeaseObservation, LeasePhase};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{api::EffectsApi, catalog::EffectId},
};

impl EffectAuthorizations {
    /// Read-only reconciliation. In-flight records are uncertain once their
    /// operation lock can be acquired. New rights facts do not erase that fact.
    pub async fn observe_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
    ) -> Result<LeaseObservation> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let lease = self
            .get(&owner, effect)?
            .ok_or(Error::State("no owned effect authorization to observe"))?;
        self.observe_locked(token, &owner, lease).await
    }
    pub(super) async fn observe_locked(
        &self,
        token: &SessionToken,
        owner: &str,
        mut lease: EffectLease,
    ) -> Result<LeaseObservation> {
        lease.phase = match lease.phase {
            LeasePhase::Registering => LeasePhase::StartUncertain,
            LeasePhase::Releasing => LeasePhase::StopUncertain,
            phase => phase,
        };
        // Clear the previous observation before I/O. A failed refresh is not a
        // new permission and must not be presented as a current successful read.
        lease.authorization = None;
        lease.recyclable = None;
        lease.last_error = None;
        lease.advance()?;
        self.session
            .with_token(token, |_| self.save(owner, &lease))?;
        let effect = lease.effect;
        let result = async {
            let auth = self
                .session
                .run_for(token, |api, _| async move {
                    EffectsApi::new(api).authorization(effect).await
                })
                .await?;
            let recycle = self
                .session
                .run_for(token, |api, _| async move {
                    EffectsApi::new(api).recyclable(effect).await
                })
                .await?;
            Ok::<_, Error>((auth, recycle))
        }
        .await;
        let (authorization, recyclable) = match result {
            Ok(value) => value,
            Err(error) => {
                lease.last_error = Some(error.to_string());
                self.save(owner, &lease)?;
                return Err(error);
            }
        };
        lease.authorization = authorization;
        lease.recyclable = recyclable;
        self.session
            .with_token(token, |_| self.save(owner, &lease))?;
        let permitted = lease
            .authorization
            .as_ref()
            .is_some_and(|auth| auth.can_use)
            && recyclable != Some(true);
        let can_use = lease.phase == LeasePhase::Registered && permitted;
        let should_release = lease.phase.needs_release() && !permitted;
        Ok(LeaseObservation {
            lease,
            can_use,
            should_release,
        })
    }
}
