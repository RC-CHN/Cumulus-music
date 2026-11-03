use super::MusicClient;
use crate::{
    Error, Result,
    download::transfer::{MediaBackend, RunOptions},
    effects::{
        authorization::{
            ActiveParametricObservation, BoundParametric, BoundParametricTarget,
            EffectAuthorizations, LeasePhase, NativeEffectReleaseReceipt,
            PendingNativeEffectRegistration, ReleaseRetry,
        },
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
        service::{
            EffectsLibrary, ParametricPreparation, PreparedParametric, StandaloneParametric,
            StandaloneParametricPreflight,
        },
    },
    playback::controller::{ParametricApplication, ParametricBindingStatus},
};
use std::sync::Arc;

/// Host-owned one-shot request. Keep it across the await: once the actor
/// dispatch begins, its exact target is available even after cancellation.
pub struct NativeParametricActivationAttempt {
    effect: EffectId,
    receipt: Option<Box<StandaloneParametric>>,
    submitted_target: Option<BoundParametricTarget>,
}

impl NativeParametricActivationAttempt {
    pub fn new(receipt: Box<StandaloneParametric>) -> Self {
        Self {
            effect: receipt.effect(),
            receipt: Some(receipt),
            submitted_target: None,
        }
    }

    pub fn effect(&self) -> EffectId {
        self.effect
    }

    /// Target for conditional local detach. Presence only means submission
    /// began; it does not prove the node was mounted or is still active.
    pub fn submitted_target(&self) -> Option<BoundParametricTarget> {
        self.submitted_target
    }
}

/// One-time actor receipt, not an ongoing observation or a PCM guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeParametricActivation {
    Configured(BoundParametricTarget),
    DeferredUntilLoaded(BoundParametricTarget),
}

impl MusicClient {
    /// Native host dependency injection for effects material transport.
    /// Catalogue/authorization APIs and production URL validation are unchanged.
    pub fn with_effect_material_backend(mut self, backend: Arc<dyn MediaBackend>) -> Self {
        self.effect_material_backend = Some(backend);
        self
    }

    fn effect_material_library(&self) -> Result<EffectsLibrary> {
        match &self.effect_material_backend {
            Some(backend) => EffectsLibrary::with_backend(self.session.clone(), backend.clone()),
            None => EffectsLibrary::new(self.session.clone()),
        }
    }

    /// Native-only preparation of one verified PEQ component. This does not
    /// register an effect, acquire a trial, or apply anything to playback.
    pub async fn prepare_native_effect_parametric(
        &self,
        epoch: &str,
        id: EffectId,
        options: RunOptions,
    ) -> Result<ParametricPreparation> {
        let token = self.player_token(epoch)?;
        self.effect_material_library()?
            .prepare_parametric_for(&token, id, options)
            .await
    }

    /// Native-only read of whether an official catalogue ID can be
    /// represented by the supported standalone PEQ plan.
    pub async fn preflight_native_effect_parametric(
        &self,
        epoch: &str,
        id: EffectId,
        options: RunOptions,
    ) -> Result<StandaloneParametricPreflight> {
        let token = self.player_token(epoch)?;
        self.effect_material_library()?
            .preflight_parametric_activation_for(&token, id, options)
            .await
    }

    /// Explicit native-only activation. Uses a fresh strict registration
    /// observation, then submits the resulting binding to the playback actor.
    /// No automatic cleanup, re-register, payment or trial is attempted.
    pub async fn activate_native_effect_parametric(
        &self,
        epoch: &str,
        attempt: &mut NativeParametricActivationAttempt,
    ) -> Result<NativeParametricActivation> {
        let token = self.player_token(epoch)?;
        let library = EffectsLibrary::new(self.session.clone())?;
        let authorization = EffectAuthorizations::new(self.session.clone())?;
        let _lock = authorization.activation_lock_for(&token)?;
        let receipt = attempt
            .receipt
            .take()
            .ok_or(Error::State("parametric activation attempt already used"))?;
        library.validate_parametric_preflight_for(&token, &receipt)?;
        let controller = self.player.controller().await?;
        let binding = authorization
            .register_preflighted_parametric_for(&library, &token, *receipt)
            .await?;
        let target = binding.target();
        attempt.submitted_target = Some(target);
        let result = controller.apply_bound_parametric(&token, binding).await;
        self.session.check_generation(&token)?;
        match result? {
            ParametricApplication::Configured => Ok(NativeParametricActivation::Configured(target)),
            ParametricApplication::DeferredUntilLoaded => {
                Ok(NativeParametricActivation::DeferredUntilLoaded(target))
            }
            _ => Err(Error::State(
                "unexpected native parametric activation receipt",
            )),
        }
    }

    /// Read the durable registration phase after an interrupted/failed
    /// activation. This is local state, not a remote occupancy observation.
    pub fn native_effect_registration_phase(
        &self,
        epoch: &str,
        effect: EffectId,
    ) -> Result<Option<LeasePhase>> {
        let token = self.player_token(epoch)?;
        Ok(EffectAuthorizations::new(self.session.clone())?
            .record_for(&token, effect)?
            .map(|lease| lease.phase))
    }

    /// Local-only list of pending native cleanup after restart.
    pub fn native_effect_pending(
        &self,
        epoch: &str,
    ) -> Result<Vec<PendingNativeEffectRegistration>> {
        let token = self.player_token(epoch)?;
        EffectAuthorizations::new(self.session.clone())?.native_pending_for(&token)
    }

    /// Review the exact durable registration before an explicit native stop.
    pub fn native_effect_release_receipt(
        &self,
        epoch: &str,
        id: EffectId,
    ) -> Result<Option<NativeEffectReleaseReceipt>> {
        let token = self.player_token(epoch)?;
        EffectAuthorizations::new(self.session.clone())?.native_release_receipt_for(&token, id)
    }

    /// Submit an explicit remote stop only when no native binding is in use.
    /// Callers must request exact local detach first when a target is known.
    pub async fn release_native_effect(
        &self,
        epoch: &str,
        receipt: &NativeEffectReleaseReceipt,
        retry: ReleaseRetry,
    ) -> Result<LeasePhase> {
        let token = self.player_token(epoch)?;
        let lease = EffectAuthorizations::new(self.session.clone())?
            .release_native_for(&token, receipt, retry)
            .await?;
        self.session.check_generation(&token)?;
        Ok(lease.phase)
    }

    /// Read and bind an already acknowledged standalone PEQ registration.
    /// This never starts registration or a trial.
    pub async fn bind_native_effect_parametric(
        &self,
        epoch: &str,
        prepared: Box<PreparedParametric>,
    ) -> Result<BoundParametric> {
        let token = self.player_token(epoch)?;
        let library = EffectsLibrary::new(self.session.clone())?;
        EffectAuthorizations::new(self.session.clone())?
            .bind_parametric_for(&library, &token, prepared)
            .await
    }

    /// Native host only: a sealed already-registered binding, supervised by
    /// the playback actor. No remote write or refresh is started here.
    pub async fn apply_native_bound_parametric(
        &self,
        epoch: &str,
        binding: BoundParametric,
    ) -> Result<crate::playback::controller::ParametricApplication> {
        let token = self.player_token(epoch)?;
        let result = self
            .player
            .controller()
            .await?
            .apply_bound_parametric(&token, binding)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

    /// Native host only: detach exactly the mounted or deferred binding.
    /// Registration cleanup is a separate explicit, journalled operation.
    pub async fn clear_native_bound_parametric(
        &self,
        epoch: &str,
        target: BoundParametricTarget,
    ) -> Result<crate::playback::controller::ParametricApplication> {
        let token = self.player_token(epoch)?;
        let result = self
            .player
            .controller()
            .await?
            .clear_bound_parametric(&token, target)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

    /// Read the local actor state of an exact target without observing the server.
    pub async fn inspect_native_bound_parametric(
        &self,
        epoch: &str,
        target: BoundParametricTarget,
    ) -> Result<ParametricBindingStatus> {
        let token = self.player_token(epoch)?;
        let status = self
            .player
            .controller()
            .await?
            .inspect_bound_parametric(&token, target)
            .await?;
        self.session.check_generation(&token)?;
        Ok(status)
    }

    /// Trusted native host trigger for a bounded read of the active binding.
    pub async fn observe_native_bound_parametric(
        &self,
        epoch: &str,
    ) -> Result<ActiveParametricObservation> {
        let token = self.player_token(epoch)?;
        let result = self
            .player
            .controller()
            .await?
            .observe_active_parametric(&token)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

    pub async fn effect_catalogue(
        &self,
        epoch: &str,
        kind: CatalogueKind,
    ) -> Result<Vec<EffectDirectoryRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .directory_for(&token, kind)
            .await
    }

    pub async fn effect_detail(
        &self,
        epoch: &str,
        id: EffectId,
    ) -> Result<Option<EffectDirectoryDetail>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .directory_detail_for(&token, id)
            .await
    }
    pub async fn effect_device_brands(&self, epoch: &str) -> Result<Vec<EffectDeviceRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .device_brands_for(&token)
            .await
    }

    pub async fn effect_device_models(
        &self,
        epoch: &str,
        brand: EffectId,
    ) -> Result<Vec<EffectDeviceRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .device_models_for(&token, brand)
            .await
    }
}
