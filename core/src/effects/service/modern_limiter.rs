//! Ordinary, non-trial material admission for the proven modern graph only.
//! A receipt confirms its input program, not permission to apply it.
use super::{EffectsLibrary, MaterialHandle, MaterialPhase, files, records::Record};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::transfer::RunOptions,
    effects::{catalog::EffectId, dsp::Format, material::ModernLimiterPlan},
    protocol::api::Api,
    storage::OperationLock,
};
use std::sync::{Arc, Weak};

const NCAE_PARAMETERS: u16 = 1;

pub enum ModernLimiterPreflight {
    Pending { bytes: u64 },
    Unsupported,
    Ready(Box<VerifiedModernLimiterMaterial>),
}

/// Opaque, native-only observation of one verified modern parameter program.
/// It carries no registration, trial, source, decoder or playback authority.
pub struct VerifiedModernLimiterMaterial {
    scope: MaterialScope,
    handle: MaterialHandle,
    attempt: String,
    plain_sha256: String,
    plan: ModernLimiterPlan,
}

impl VerifiedModernLimiterMaterial {
    pub fn effect(&self) -> EffectId {
        self.handle.effect
    }

    /// Geometry for the offline DSP graph; does not validate the account or
    /// open, decode or attach a playback source.
    pub fn supports_graph_format(&self, format: Format) -> bool {
        self.plan.supports_format(format)
    }

    pub(crate) fn require_live_format(&self, format: Format) -> Result<()> {
        if !self.supports_graph_format(format) {
            return Err(Error::State(
                "owned source geometry is unsupported for modern effect",
            ));
        }
        self.plan.prepare_control(format, false)?;
        Ok(())
    }

    pub(crate) fn revision(&self) -> &str {
        self.scope.revision()
    }

    fn ready_record(&self, library: &EffectsLibrary, token: &SessionToken) -> Result<Record> {
        library
            .record(token, &self.handle)?
            .filter(|record| {
                record.phase == MaterialPhase::Ready
                    && record.attempt == self.attempt
                    && record.plain_sha256.as_deref() == Some(self.plain_sha256.as_str())
            })
            .ok_or(Error::State(
                "verified modern effect material is no longer available",
            ))
    }
}

impl EffectsLibrary {
    /// Read current PC detail and rights, then verify and classify only the
    /// first parameter material. No auth/set, auth/stop or audio operation.
    pub async fn preflight_modern_limiter_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        options: RunOptions,
    ) -> Result<ModernLimiterPreflight> {
        if effect.interests_type() != 127 {
            return Err(Error::InvalidInput(
                "special effects require their own material source",
            ));
        }
        let owner = self.owner(token)?;
        let scope = MaterialScope::capture(&self.session, token)?;
        let outcome = self
            .acquire_ordinary_material_for(token, effect, options, true, Some(&scope))
            .await?;
        scope.current()?;
        let Some(material) = outcome.material else {
            return Ok(ModernLimiterPreflight::Pending {
                bytes: outcome.bytes,
            });
        };
        if material.type_id != NCAE_PARAMETERS {
            return Ok(ModernLimiterPreflight::Unsupported);
        }
        let Ok(plan) = ModernLimiterPlan::from_material(&material.parameters) else {
            // Verified NCAE/JSON but no supported graph. No legacy fallback,
            // inferred defaults or standalone PEQ registration.
            scope.current()?;
            return Ok(ModernLimiterPreflight::Unsupported);
        };
        let receipt = VerifiedModernLimiterMaterial {
            scope,
            handle: outcome.handle,
            attempt: outcome.transfer_id,
            plain_sha256: material.plain_sha256,
            plan,
        };
        let _lock = self.lock(&owner, &receipt.handle.lock_key())?;
        receipt.ready_record(self, token)?;
        receipt.scope.current()?;
        Ok(ModernLimiterPreflight::Ready(Box::new(receipt)))
    }

    /// Reverify the same complete transfer on a blocking worker, in the same
    /// account generation and credential revision. Does not read server
    /// rights again and cannot serve as an activation or registration proof.
    pub async fn validate_modern_limiter_preflight_for(
        &self,
        token: &SessionToken,
        receipt: &VerifiedModernLimiterMaterial,
    ) -> Result<()> {
        self.hold_modern_limiter_material(token, receipt)
            .await
            .map(drop)
    }

    pub(crate) async fn hold_modern_limiter_material(
        &self,
        token: &SessionToken,
        receipt: &VerifiedModernLimiterMaterial,
    ) -> Result<OperationLock> {
        receipt.scope.check(&self.session, token)?;
        let owner = self.owner(token)?;
        let lock = self.lock(&owner, &receipt.handle.lock_key())?;
        self.verify_locked_modern_limiter_material(token, receipt)
            .await?;
        Ok(lock)
    }

    pub(crate) fn check_modern_limiter_material(
        &self,
        token: &SessionToken,
        receipt: &VerifiedModernLimiterMaterial,
    ) -> Result<()> {
        self.current_modern_limiter_record(token, receipt).map(drop)
    }

    fn current_modern_limiter_record(
        &self,
        token: &SessionToken,
        receipt: &VerifiedModernLimiterMaterial,
    ) -> Result<Record> {
        receipt.scope.check(&self.session, token)?;
        let record = receipt.ready_record(self, token)?;
        receipt.scope.current()?;
        Ok(record)
    }

    pub(crate) async fn verify_locked_modern_limiter_material(
        &self,
        token: &SessionToken,
        receipt: &VerifiedModernLimiterMaterial,
    ) -> Result<()> {
        let owner = self.owner(token)?;
        let record = self.current_modern_limiter_record(token, receipt)?;
        let child = self.ready_material_transfer(&owner, &record)?;
        let material = tokio::task::spawn_blocking(move || files::verify(&child))
            .await
            .map_err(|_| Error::State("effect material verification worker stopped"))?;
        receipt.scope.current()?;
        let material = material?;
        if material.type_id != NCAE_PARAMETERS || material.plain_sha256 != receipt.plain_sha256 {
            return Err(Error::Media(
                "verified modern effect material digest changed",
            ));
        }
        Ok(())
    }
}

/// Pin the exact credential revision at every read and transfer checkpoint,
/// without keeping a replaced account or its transport alive in a receipt.
#[derive(Clone)]
pub(super) struct MaterialScope {
    session: Weak<AccountSession>,
    token: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
}

impl MaterialScope {
    fn capture(session: &Arc<AccountSession>, token: &SessionToken) -> Result<Self> {
        let api = session.native_api(token)?;
        require_verified_identity(session, token, &api)?;
        let revision = api
            .effect_catalogue_credential_revision()?
            .filter(|revision| !revision.is_empty())
            .ok_or(Error::State(
                "scoped effect material credentials are unavailable",
            ))?;
        let scope = Self {
            session: Arc::downgrade(session),
            token: token.clone(),
            api: Arc::downgrade(&api),
            revision,
        };
        scope.current()?;
        Ok(scope)
    }

    pub(super) fn revision(&self) -> &str {
        &self.revision
    }

    fn check(&self, session: &Arc<AccountSession>, token: &SessionToken) -> Result<()> {
        if &self.token != token || !Weak::ptr_eq(&self.session, &Arc::downgrade(session)) {
            return Err(Error::StaleResponse);
        }
        self.current()
    }

    pub(super) fn current(&self) -> Result<()> {
        let session = self.session.upgrade().ok_or(Error::StaleResponse)?;
        let api = self.api.upgrade().ok_or(Error::StaleResponse)?;
        require_verified_identity(&session, &self.token, &api)?;
        if api.effect_catalogue_credential_revision()?.as_deref() != Some(self.revision.as_str()) {
            return Err(Error::CredentialChanged);
        }
        require_verified_identity(&session, &self.token, &api)
    }
}

fn require_verified_identity(
    session: &AccountSession,
    token: &SessionToken,
    api: &Arc<dyn Api>,
) -> Result<()> {
    session.with_pc_presentation_identity(token, api, |uid, _, _| {
        uid.map(|_| ()).ok_or(Error::StaleResponse)
    })
}
