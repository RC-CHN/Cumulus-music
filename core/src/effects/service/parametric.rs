//! Source-backed native preparation only. Download permission does not
//! acknowledge activation, an authorization lease, or trial consent.
use super::{EffectsLibrary, MaterialHandle, MaterialPhase};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::transfer::RunOptions,
    effects::{
        catalog::EffectId,
        material::{Component, MaterialSchema, peq_calls, route_sections},
        parametric::ParametricPlan,
    },
};
use std::sync::{Arc, Weak};

pub enum ParametricPreparation {
    Pending { bytes: u64 },
    MissingComponent { schema: MaterialSchema },
    Prepared(Box<PreparedParametric>),
}

/// Read-only eligibility for the supported standalone component. Ready
/// is a material receipt, not a lease or an audio-engine activation receipt.
pub enum StandaloneParametricPreflight {
    Pending { bytes: u64 },
    Unsupported,
    Ready(Box<StandaloneParametric>),
}

pub struct StandaloneParametric(pub(in crate::effects) PreparedParametric);

impl StandaloneParametric {
    pub fn effect(&self) -> EffectId {
        self.0.effect()
    }
}

/// Native provenance for one component, never a serializable preset or lease.
pub struct PreparedParametric {
    session: Weak<AccountSession>,
    token: SessionToken,
    handle: MaterialHandle,
    plain_sha256: String,
    plan: ParametricPlan,
    additional_sections: Vec<Component>,
}

impl PreparedParametric {
    pub fn effect(&self) -> EffectId {
        self.handle.effect
    }
    pub fn enabled(&self) -> bool {
        self.plan.enabled()
    }
    pub(in crate::effects) fn is_standalone(&self) -> bool {
        self.enabled() && self.additional_sections.is_empty()
    }
    /// Other sections dispatched by EFB760, including sections that turn a
    /// node off. No claim is made that they are playable or that PEQ suffices.
    pub fn additional_sections(&self) -> &[Component] {
        &self.additional_sections
    }
}

impl EffectsLibrary {
    /// Acquire/revalidate the ordinary PC effect's first parameter material,
    /// then select the exact recovered PEQ branch. All network calls have read
    /// semantics; no registration, release, purchase or playback is performed.
    pub async fn prepare_parametric_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        options: RunOptions,
    ) -> Result<ParametricPreparation> {
        self.prepare_parametric_with_rights(token, effect, options, false)
            .await
    }

    /// Accept only a fresh non-trial material containing an enabled PEQ and
    /// no other routed component. Even an explicit off section is excluded.
    pub async fn preflight_parametric_activation_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        options: RunOptions,
    ) -> Result<StandaloneParametricPreflight> {
        let prepared = self
            .prepare_parametric_with_rights(token, effect, options, true)
            .await?;
        match prepared {
            ParametricPreparation::Pending { bytes } => {
                Ok(StandaloneParametricPreflight::Pending { bytes })
            }
            ParametricPreparation::Prepared(prepared) if prepared.is_standalone() => {
                self.prepared_parametric_for(token, &prepared)?;
                Ok(StandaloneParametricPreflight::Ready(Box::new(
                    StandaloneParametric(*prepared),
                )))
            }
            _ => Ok(StandaloneParametricPreflight::Unsupported),
        }
    }

    /// Revalidate the opaque receipt before a subsequent native step.
    /// No registration or fresh server authorization is implied.
    pub fn validate_parametric_preflight_for(
        &self,
        token: &SessionToken,
        preflight: &StandaloneParametric,
    ) -> Result<()> {
        self.with_prepared_parametric_for(token, &preflight.0, |_| Ok(()))
    }

    async fn prepare_parametric_with_rights(
        &self,
        token: &SessionToken,
        effect: EffectId,
        options: RunOptions,
        require_non_trial: bool,
    ) -> Result<ParametricPreparation> {
        if effect.interests_type() != 127 {
            return Err(Error::InvalidInput(
                "special effects require their own material source",
            ));
        }
        let outcome = self
            .acquire_with_rights(token, effect, options, require_non_trial)
            .await?;
        let result = if let Some(material) = outcome.material {
            let routing = route_sections(&material.parameters)?;
            if let Some(section) = routing
                .sections
                .iter()
                .find(|entry| entry.component == Component::Peq)
            {
                let plan = ParametricPlan::from_calls(&peq_calls(section.section)?)?;
                ParametricPreparation::Prepared(Box::new(PreparedParametric {
                    session: Arc::downgrade(&self.session),
                    token: token.clone(),
                    handle: outcome.handle,
                    plain_sha256: material.plain_sha256,
                    plan,
                    additional_sections: routing
                        .sections
                        .iter()
                        .map(|entry| entry.component)
                        .filter(|component| *component != Component::Peq)
                        .collect(),
                }))
            } else {
                ParametricPreparation::MissingComponent {
                    schema: routing.schema,
                }
            }
        } else {
            ParametricPreparation::Pending {
                bytes: outcome.bytes,
            }
        };
        self.session.check_generation(token)?;
        Ok(result)
    }

    /// Recheck the source receipt when preparing a later native activation.
    /// A returned plan is still only data: it confers no activation authority
    /// and callers must separately enforce live lease ownership and rights.
    pub fn prepared_parametric_for(
        &self,
        token: &SessionToken,
        prepared: &PreparedParametric,
    ) -> Result<ParametricPlan> {
        self.with_prepared_parametric_for(token, prepared, Ok)
    }

    pub(in crate::effects) fn with_prepared_parametric_for<T>(
        &self,
        token: &SessionToken,
        prepared: &PreparedParametric,
        action: impl FnOnce(ParametricPlan) -> Result<T>,
    ) -> Result<T> {
        if token != &prepared.token
            || !prepared
                .session
                .upgrade()
                .is_some_and(|session| Arc::ptr_eq(&self.session, &session))
        {
            return Err(Error::StaleResponse);
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner, &prepared.handle.lock_key())?;
        let record = self.record(token, &prepared.handle)?;
        if !record.is_some_and(|record| {
            record.phase == MaterialPhase::Ready
                && record.plain_sha256.as_deref() == Some(prepared.plain_sha256.as_str())
        }) {
            return Err(Error::State(
                "prepared effect material is no longer available",
            ));
        }
        self.session.check_generation(token)?;
        action(prepared.plan)
    }
}
