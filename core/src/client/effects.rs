use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    download::transfer::{MediaBackend, RunOptions},
    effects::{
        RememberedHeadphone,
        authorization::{
            ActiveParametricObservation, BoundParametric, BoundParametricTarget,
            EffectAuthorizations, LeasePhase, NativeEffectReleaseReceipt,
            PendingNativeEffectRegistration, ReleaseRetry,
        },
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
        service::{
            DeviceMaterialCheck, EffectsLibrary, ParametricPreparation, PreparedParametric,
            StandaloneParametric, StandaloneParametricPreflight,
        },
    },
    playback::controller::{
        DeviceParametricComparison, DeviceParametricTarget, DeviceRestoreAuthority,
        ParametricApplication, ParametricBindingStatus, PlaybackController,
    },
};
use std::sync::Arc;

/// Host-owned one-shot request. Keep it across the await: once the actor
/// dispatch begins, its exact target is available even after cancellation.
pub struct NativeParametricActivationAttempt {
    effect: EffectId,
    source: Option<ParametricActivationSource>,
    submitted_target: Option<BoundParametricTarget>,
}

enum ParametricActivationSource {
    Preflight(Box<StandaloneParametric>),
    Existing,
}

impl NativeParametricActivationAttempt {
    pub fn new(receipt: Box<StandaloneParametric>) -> Self {
        Self {
            effect: receipt.effect(),
            source: Some(ParametricActivationSource::Preflight(receipt)),
            submitted_target: None,
        }
    }

    pub fn existing(effect: EffectId) -> Self {
        Self {
            effect,
            source: Some(ParametricActivationSource::Existing),
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
        self.ensure_no_native_device_parametric(epoch).await?;
        let Some(ParametricActivationSource::Preflight(receipt)) = attempt.source.take() else {
            return Err(Error::State("parametric activation attempt unavailable"));
        };
        library.validate_parametric_preflight_for(&token, &receipt)?;
        let controller = self.player.controller().await?;
        let binding = authorization
            .register_preflighted_parametric_for(&library, &token, *receipt)
            .await?;
        self.submit_effect_binding(controller, &token, attempt, binding)
            .await
    }

    /// Explicit resume: bind only the exact old registration; never register
    /// or release on a missing, changed or uncertain receipt.
    pub async fn resume_native_effect_parametric(
        &self,
        epoch: &str,
        attempt: &mut NativeParametricActivationAttempt,
        existing: &NativeEffectReleaseReceipt,
    ) -> Result<NativeParametricActivation> {
        let token = self.player_token(epoch)?;
        let library = self.effect_material_library()?;
        let authorization = EffectAuthorizations::new(self.session.clone())?;
        let _activation = authorization.activation_lock_for(&token)?;
        self.ensure_no_native_device_parametric(epoch).await?;
        if !matches!(
            attempt.source.take(),
            Some(ParametricActivationSource::Existing)
        ) || attempt.effect != existing.effect()
        {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        let binding = {
            // Do not advance a registration revision still in use elsewhere.
            // The actor takes its own fence under the same activation lock.
            let _unused = authorization.acquire_bound_use_fence_for(&token)?;
            if authorization
                .native_pending_for(&token)?
                .iter()
                .any(|entry| entry.id != attempt.effect)
            {
                return Err(Error::MutationBusy);
            }
            authorization.check_preflighted_registration(&token, existing)?;
            let receipt = match library
                .preflight_parametric_activation_for(&token, attempt.effect, Default::default())
                .await?
            {
                StandaloneParametricPreflight::Ready(receipt) => receipt,
                StandaloneParametricPreflight::Pending { .. } => {
                    return Err(Error::State("effect material is not ready"));
                }
                StandaloneParametricPreflight::Unsupported => {
                    return Err(Error::State(
                        "effect is not a standalone enabled PEQ material",
                    ));
                }
            };
            authorization
                .bind_preflighted_registration_for(&library, &token, *receipt, existing)
                .await?
        };
        self.submit_effect_binding(controller, &token, attempt, binding)
            .await
    }

    async fn submit_effect_binding(
        &self,
        controller: &PlaybackController,
        token: &SessionToken,
        attempt: &mut NativeParametricActivationAttempt,
        binding: BoundParametric,
    ) -> Result<NativeParametricActivation> {
        let target = binding.target();
        attempt.submitted_target = Some(target);
        let result = controller.apply_bound_parametric(token, binding).await;
        self.session.check_generation(token)?;
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

    pub async fn effect_device_material(
        &self,
        epoch: &str,
        brand: EffectId,
        model: EffectId,
    ) -> Result<DeviceMaterialCheck> {
        let token = self.player_token(epoch)?;
        self.effect_material_library()?
            .check_device_material_for(&token, brand, model, RunOptions::default())
            .await
    }

    /// Install only the freshly resolved, verified standalone PEQ of this model.
    /// Headphone tuning does not register a catalogue entitlement or select an output.
    pub async fn apply_native_device_parametric(
        &self,
        epoch: &str,
        brand: EffectId,
        model: EffectId,
    ) -> Result<ParametricApplication> {
        let token = self.player_token(epoch)?;
        self.apply_device_parametric_for(&token, brand, model, None)
            .await
    }

    /// This account-scoped lookup never starts a player or requests a catalogue.
    pub fn remembered_native_headphone(&self, epoch: &str) -> Result<Option<RememberedHeadphone>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |owner| {
            RememberedHeadphone::read(&self.registry.store(), owner)
        })
    }

    /// Explicitly revalidate a saved choice for a settled native player.
    /// This does not initialize audio or start/resume playback.
    pub async fn restore_native_headphone(
        &self,
        epoch: &str,
        revision: &str,
    ) -> Result<ParametricApplication> {
        let token = self.player_token(epoch)?;
        let remembered = self
            .remembered_native_headphone(epoch)?
            .filter(|value| value.revision == revision)
            .ok_or(Error::StaleResponse)?;
        let current = self
            .player
            .controller
            .get()
            .map(|player| player.snapshot())
            .filter(|value| value.account == token && value.headphone_restore_ready())
            .ok_or(Error::State("player not ready for headphone restore"))?;
        self.apply_device_parametric_for(
            &token,
            remembered.brand,
            remembered.model,
            Some(DeviceRestoreAuthority {
                playback: current.fence(),
                remembered,
            }),
        )
        .await
    }

    async fn apply_device_parametric_for(
        &self,
        token: &SessionToken,
        brand: EffectId,
        model: EffectId,
        restore: Option<DeviceRestoreAuthority>,
    ) -> Result<ParametricApplication> {
        let authorization = EffectAuthorizations::new(self.session.clone())?;
        let _activation = authorization.activation_lock_for(token)?;
        let controller = if let Some(expected) = &restore {
            let player = self
                .player
                .controller
                .get()
                .ok_or(Error::StaleResponse)?
                .clone();
            let state = player.snapshot();
            if state.account != *token
                || state.fence() != expected.playback
                || self
                    .session
                    .with_token(token, |owner| {
                        RememberedHeadphone::read(&self.registry.store(), owner)
                    })?
                    .as_ref()
                    != Some(&expected.remembered)
            {
                return Err(Error::StaleResponse);
            }
            if state
                .device_parametric
                .is_some_and(|view| !view.status.is_detached())
            {
                return Err(Error::State(
                    "clear headphone tuning before applying another parametric effect",
                ));
            }
            player
        } else {
            self.ensure_no_native_device_parametric(&token.generation.to_string())
                .await?;
            self.player.controller().await?.clone()
        };
        if !authorization.native_pending_for(token)?.is_empty() {
            return Err(Error::State(
                "stop the registered parametric effect before applying headphone tuning",
            ));
        }
        let (_, prepared) = self
            .effect_material_library()?
            .prepare_device_parametric_for(token, brand, model, RunOptions::default())
            .await?;
        let prepared = prepared.ok_or(Error::State(
            "headphone tuning is not a verified standalone parametric effect",
        ))?;
        self.session.check_generation(token)?;
        let result = controller
            .apply_device_parametric(
                token,
                DeviceParametricTarget { brand, model },
                prepared.plan,
                prepared.model_name,
                restore,
            )
            .await?;
        self.session.check_generation(token)?;
        Ok(result)
    }

    pub async fn clear_native_device_parametric(
        &self,
        epoch: &str,
        brand: EffectId,
        model: EffectId,
        activation_id: &str,
    ) -> Result<ParametricApplication> {
        let token = self.player_token(epoch)?;
        let id = parse_headphone_identity(activation_id)?;
        let result = self
            .player
            .controller()
            .await?
            .clear_device_parametric(&token, DeviceParametricTarget { brand, model }, id)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

    pub async fn compare_native_device_parametric(
        &self,
        epoch: &str,
        target: DeviceParametricTarget,
        activation_id: &str,
        play_id: &str,
        receipt: &str,
        paused: bool,
    ) -> Result<ParametricApplication> {
        let token = self.player_token(epoch)?;
        let activation_id = parse_headphone_identity(activation_id)?;
        let _ = parse_headphone_identity(play_id)?;
        let comparison = DeviceParametricComparison {
            play_id: play_id.to_owned(),
            receipt: parse_headphone_identity(receipt)?,
        };
        let authorization = EffectAuthorizations::new(self.session.clone())?;
        let _activation = authorization.activation_lock_for(&token)?;
        // Comparison can only address an already loaded player. A stale
        // command cannot lazily start the player or resolve an online source.
        let result = self
            .player
            .controller
            .get()
            .ok_or(Error::StaleResponse)?
            .compare_device_parametric(&token, target, activation_id, comparison, paused)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
    }

    pub async fn ensure_no_native_device_parametric(&self, epoch: &str) -> Result<()> {
        if self
            .player_snapshot(epoch)
            .await?
            .device_parametric
            .is_some_and(|view| !view.status.is_detached())
        {
            return Err(Error::State(
                "clear headphone tuning before applying another parametric effect",
            ));
        }
        Ok(())
    }
}

fn parse_headphone_identity(value: &str) -> Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value)
        .ok()
        .filter(|id| id.simple().to_string() == value)
        .ok_or(Error::InvalidInput("invalid headphone activation identity"))
}
