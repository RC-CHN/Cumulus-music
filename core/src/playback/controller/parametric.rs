use super::{Message, PlaybackController, PlaybackFence, state::Actor};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{
        RememberedHeadphone,
        authorization::{
            ActiveParametricObservation, BoundParametric, BoundParametricTarget,
            EffectAuthorizations,
        },
        catalog::EffectId,
        parametric::ParametricPlan,
    },
    playback::engine::{AudioEngine, Cancellation, EngineSnapshot},
    storage::OperationLock,
};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;

/// Configuration acknowledgement, not a claim of observed PCM processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParametricApplication {
    Cleared,
    Configured,
    BypassConfigured,
    DeferredUntilLoaded,
}

/// Actor-local status of a specific registration binding; not a PCM receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParametricBindingStatus {
    Absent,
    Deferred,
    Configured,
    DetachRequired,
    ProcessorFailed,
}

impl ParametricBindingStatus {
    /// Both states confirm no native component for the exact target.
    pub fn is_detached(self) -> bool {
        matches!(self, Self::Absent | Self::ProcessorFailed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DeviceParametricTarget {
    pub brand: EffectId,
    pub model: EffectId,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DeviceParametricView {
    #[serde(flatten)]
    pub target: DeviceParametricTarget,
    #[serde(
        rename = "activationId",
        serialize_with = "serialize_device_activation"
    )]
    pub activation_id: uuid::Uuid,
    #[serde(rename = "modelName", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub status: ParametricBindingStatus,
    #[serde(rename = "selectionSaveFailed")]
    pub selection_save_failed: bool,
}

fn serialize_device_activation<S: serde::Serializer>(
    id: &uuid::Uuid,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&id.simple().to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DeviceActivationId {
    target: DeviceParametricTarget,
    id: uuid::Uuid,
}

#[derive(Clone)]
struct DeviceActivation {
    identity: DeviceActivationId,
    model_name: Option<String>,
}

pub(crate) struct DeviceRestoreAuthority {
    pub playback: PlaybackFence,
    pub remembered: RememberedHeadphone,
}

pub(super) struct InspectRequest {
    token: SessionToken,
    target: BoundParametricTarget,
    reply: oneshot::Sender<Result<ParametricBindingStatus>>,
}

pub(super) struct Request {
    token: SessionToken,
    selection: Selection,
    reply: oneshot::Sender<Result<ParametricApplication>>,
}

pub(super) struct ObserveRequest {
    token: SessionToken,
    reply: oneshot::Sender<Result<ActiveParametricObservation>>,
}

enum Selection {
    Plain(Option<Box<ParametricPlan>>),
    Bound(Arc<BoundParametric>),
    ClearBound(BoundParametricTarget),
    DeviceRequest(
        DeviceParametricTarget,
        Box<ParametricPlan>,
        Option<String>,
        Option<DeviceRestoreAuthority>,
    ),
    Device(DeviceActivation, Box<ParametricPlan>),
    ClearDevice(DeviceActivationId),
}

#[derive(Default)]
pub(super) struct State {
    deferred: Option<Selection>,
    applied: Option<Arc<BoundParametric>>,
    // A failed replacement may leave either the previous or attempted node.
    installing: Option<Arc<BoundParametric>>,
    bound_use: Option<OperationLock>,
    cleared: VecDeque<(SessionToken, BoundParametricTarget)>,
    possibly_attached: bool,
    detach_required: bool,
    failed_binding: Option<(SessionToken, BoundParametricTarget)>,
    device: Option<DeviceActivation>,
    failed_device: Option<DeviceActivation>,
    device_selection_save_failed: bool,
}

impl State {
    fn deferred_device(&self) -> Option<&DeviceActivation> {
        match &self.deferred {
            Some(Selection::Device(target, _)) => Some(target),
            _ => None,
        }
    }

    pub(super) fn device_status(&self) -> Option<DeviceParametricView> {
        use ParametricBindingStatus as Status;
        let (activation, status) = if let Some(target) = &self.device {
            (
                target,
                if self.detach_required {
                    Status::DetachRequired
                } else {
                    Status::Configured
                },
            )
        } else if let Some(target) = self.deferred_device() {
            (target, Status::Deferred)
        } else {
            (self.failed_device.as_ref()?, Status::ProcessorFailed)
        };
        Some(DeviceParametricView {
            target: activation.identity.target,
            activation_id: activation.identity.id,
            model_name: activation.model_name.clone(),
            status,
            selection_save_failed: self.device_selection_save_failed,
        })
    }

    pub(super) fn bound_status(&self) -> Option<ParametricBindingStatus> {
        use ParametricBindingStatus as Status;
        if self.installing.is_some()
            || (self.detach_required && self.applied.is_some())
            || self
                .applied
                .as_ref()
                .is_some_and(|binding| binding.requires_detach())
        {
            Some(Status::DetachRequired)
        } else if self.applied.is_some() {
            Some(Status::Configured)
        } else if matches!(self.deferred, Some(Selection::Bound(_))) {
            Some(Status::Deferred)
        } else if self.failed_binding.is_some() {
            Some(Status::ProcessorFailed)
        } else {
            None
        }
    }

    fn record_clear(&mut self, token: &SessionToken, binding: &BoundParametric) {
        const MAX_RECENT_DETACHES: usize = 8;
        binding.retire();
        if self.cleared.len() == MAX_RECENT_DETACHES {
            self.cleared.pop_front();
        }
        self.cleared.push_back((token.clone(), binding.target()));
    }

    fn clear_deferred(&mut self, token: &SessionToken) {
        if let Some(Selection::Bound(binding)) = self.deferred.take() {
            self.record_clear(token, &binding);
        }
    }

    fn release_unused_fence(&mut self) {
        if self.applied.is_none()
            && self.installing.is_none()
            && !matches!(self.deferred, Some(Selection::Bound(_)))
        {
            self.bound_use = None;
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if (self.applied.is_some() || self.installing.is_some())
            && let Some(fence) = self.bound_use.take()
        {
            // Aborted or failed shutdown can leave an engine worker alive.
            // Keep the OS fence until process exit instead of claiming absence.
            std::mem::forget(fence);
        }
    }
}

impl PlaybackController {
    pub(crate) async fn apply_device_parametric(
        &self,
        token: &SessionToken,
        target: DeviceParametricTarget,
        plan: ParametricPlan,
        model_name: Option<String>,
        restore: Option<DeviceRestoreAuthority>,
    ) -> Result<ParametricApplication> {
        self.request_parametric(
            token,
            Selection::DeviceRequest(target, Box::new(plan), model_name, restore),
        )
        .await
    }

    pub(crate) async fn clear_device_parametric(
        &self,
        token: &SessionToken,
        target: DeviceParametricTarget,
        activation_id: uuid::Uuid,
    ) -> Result<ParametricApplication> {
        self.request_parametric(
            token,
            Selection::ClearDevice(DeviceActivationId {
                target,
                id: activation_id,
            }),
        )
        .await
    }

    /// Set one explicitly selected native PEQ component for this playback
    /// session. Nothing is persisted or automatically restored. A deferred
    /// request is superseded by another setting or discarded if playback stops.
    pub async fn set_parametric_component(
        &self,
        token: &SessionToken,
        plan: Option<ParametricPlan>,
    ) -> Result<ParametricApplication> {
        self.request_parametric(token, Selection::Plain(plan.map(Box::new)))
            .await
    }

    /// Apply an opaque, already registered component with runtime local
    /// journal supervision. No registration or rights requests are made here.
    pub async fn apply_bound_parametric(
        &self,
        token: &SessionToken,
        binding: BoundParametric,
    ) -> Result<ParametricApplication> {
        self.request_parametric(token, Selection::Bound(Arc::new(binding)))
            .await
    }

    /// Remove only the exact bound component, without sending a server stop.
    pub async fn clear_bound_parametric(
        &self,
        token: &SessionToken,
        target: BoundParametricTarget,
    ) -> Result<ParametricApplication> {
        self.request_parametric(token, Selection::ClearBound(target))
            .await
    }

    pub async fn inspect_bound_parametric(
        &self,
        token: &SessionToken,
        target: BoundParametricTarget,
    ) -> Result<ParametricBindingStatus> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Message::InspectParametric(InspectRequest {
                token: token.clone(),
                target,
                reply,
            }))
            .map_err(|_| Error::State("player is closed"))?;
        result.await.map_err(|_| Error::Cancelled)?
    }

    /// Explicit bounded server observation of the active bound component.
    /// No caller-selected ID or automatic registration/release.
    pub async fn observe_active_parametric(
        &self,
        token: &SessionToken,
    ) -> Result<ActiveParametricObservation> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Message::ObserveParametric(ObserveRequest {
                token: token.clone(),
                reply,
            }))
            .map_err(|_| Error::State("player is closed"))?;
        result.await.map_err(|_| Error::Cancelled)?
    }

    async fn request_parametric(
        &self,
        token: &SessionToken,
        selection: Selection,
    ) -> Result<ParametricApplication> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Message::Parametric(Box::new(Request {
                token: token.clone(),
                selection,
                reply,
            })))
            .map_err(|_| Error::State("player is closed"))?;
        result.await.map_err(|_| Error::Cancelled)?
    }
}

impl Actor {
    pub(super) fn parametric_inspect(&self, request: InspectRequest) {
        let result = self.session.check_generation(&request.token).map(|_| {
            use ParametricBindingStatus as Status;
            let target = request.target;
            if request.token != self.token {
                return Status::Absent;
            }
            if matches!(&self.parametric.deferred, Some(Selection::Bound(binding)) if binding.target() == target) {
                return Status::Deferred;
            }
            if let Some(binding) = self.parametric.applied.as_ref().filter(|b| b.target() == target) {
                return if !self.parametric.detach_required
                    && binding.with_current(&self.session, &request.token, |_| Ok(())).is_ok()
                {
                    Status::Configured
                } else {
                    Status::DetachRequired
                };
            }
            if self.parametric.installing.as_ref().is_some_and(|b| b.target() == target) {
                return Status::DetachRequired;
            }
            if self.parametric.failed_binding.as_ref().is_some_and(|(token, failed)| token == &request.token && *failed == target) {
                return Status::ProcessorFailed;
            }
            Status::Absent
        });
        let _ = request.reply.send(result);
    }

    pub(super) async fn parametric_observe(&mut self, request: ObserveRequest) {
        if request.reply.is_closed() {
            return;
        }
        if let Err(error) = self.rebind().await {
            let _ = request.reply.send(Err(error));
        } else if request.token != self.token {
            let _ = request.reply.send(Err(Error::StaleResponse));
        } else if let Some(binding) = self.parametric.applied.clone() {
            // The bounded task does not hold the actor or audio lock. Once
            // dispatched it can mark a denial even if the requester drops.
            tokio::spawn(async move {
                let outcome = binding.observe_active(&request.token).await;
                let _ = request.reply.send(outcome);
            });
        } else {
            let _ = request
                .reply
                .send(Ok(ActiveParametricObservation::NoActiveBinding));
        }
    }

    pub(super) async fn parametric_request(&mut self, request: Request) {
        if request.reply.is_closed() {
            return;
        }
        let result = self
            .parametric_change(&request.token, request.selection)
            .await;
        let _ = request.reply.send(result);
    }

    async fn parametric_change(
        &mut self,
        token: &SessionToken,
        selection: Selection,
    ) -> Result<ParametricApplication> {
        self.rebind().await?;
        if !self.bound || token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.check_generation(token)?;
        if let Selection::ClearBound(target) = selection {
            return self.parametric_clear_bound(target).await;
        }
        if let Selection::ClearDevice(target) = selection {
            return self.parametric_clear_device(target).await;
        }
        if let Selection::DeviceRequest(_, _, _, Some(expected)) = &selection {
            // A restore should never adopt a replacement queue or start a
            // decoder. Reload only the stored queue to verify its identity.
            self.queue.reload()?;
            if !self.state.active()
                || self.pending.is_some()
                || self.snapshot().fence() != expected.playback
                || self
                    .session
                    .with_token(token, |owner| RememberedHeadphone::read(&self.store, owner))?
                    .as_ref()
                    != Some(&expected.remembered)
            {
                return Err(Error::StaleResponse);
            }
        }
        if self.parametric.device.is_some()
            || self.parametric.deferred_device().is_some()
            || (matches!(&selection, Selection::DeviceRequest(..))
                && (self.parametric.possibly_attached || self.parametric.deferred.is_some()))
        {
            return Err(Error::State(
                "clear the current parametric effect before applying another",
            ));
        }
        if let Selection::Bound(binding) = &selection {
            let fence = if self.parametric.bound_use.is_none() {
                Some(
                    EffectAuthorizations::new(self.session.clone())?
                        .acquire_bound_use_fence_for(token)?,
                )
            } else {
                None
            };
            binding.with_current(&self.session, token, |_| Ok(()))?;
            if let Some(fence) = fence {
                self.parametric.bound_use = Some(fence);
            }
        }
        let selection = match selection {
            Selection::DeviceRequest(target, plan, model_name, _) => Selection::Device(
                DeviceActivation {
                    identity: DeviceActivationId {
                        target,
                        id: uuid::Uuid::new_v4(),
                    },
                    model_name,
                },
                plan,
            ),
            other => other,
        };
        if self.pending.is_some() {
            // Do not wait on the engine mutex held by a decoder. The latest
            // request will run in loaded(), before its first unpause.
            self.parametric.clear_deferred(token);
            self.parametric.deferred = Some(selection);
            self.parametric.release_unused_fence();
            return Ok(ParametricApplication::DeferredUntilLoaded);
        }
        self.parametric_apply_selection(selection).await
    }

    async fn parametric_clear_device(
        &mut self,
        target: DeviceActivationId,
    ) -> Result<ParametricApplication> {
        let is_target = |activation: &DeviceActivation| activation.identity == target;
        if self.parametric.deferred_device().is_some_and(is_target) {
            self.parametric.deferred = None;
        } else if self.parametric.device.as_ref().is_some_and(is_target) {
            if self.pending.is_some() {
                return Err(Error::MutationBusy);
            }
            return self.parametric_apply(None).await;
        } else if self
            .parametric
            .failed_device
            .as_ref()
            .is_some_and(is_target)
        {
            self.parametric.failed_device = None;
        } else {
            return Err(Error::StaleResponse);
        }
        Ok(ParametricApplication::Cleared)
    }

    async fn parametric_clear_bound(
        &mut self,
        target: BoundParametricTarget,
    ) -> Result<ParametricApplication> {
        if self
            .parametric
            .failed_binding
            .as_ref()
            .is_some_and(|(token, failed)| token == &self.token && *failed == target)
        {
            self.parametric.failed_binding = None;
            return Ok(ParametricApplication::Cleared);
        }
        if matches!(&self.parametric.deferred, Some(Selection::Bound(binding)) if binding.target() == target)
        {
            self.parametric.clear_deferred(&self.token);
            self.parametric.release_unused_fence();
            return Ok(ParametricApplication::Cleared);
        }
        if self
            .parametric
            .cleared
            .iter()
            .any(|(token, cleared)| token == &self.token && *cleared == target)
        {
            return Ok(ParametricApplication::Cleared);
        }
        if !self
            .parametric
            .applied
            .as_ref()
            .is_some_and(|binding| binding.target() == target)
        {
            return Err(Error::StaleResponse);
        }
        // A loader can own the engine mutex. Do not claim detachment or
        // supersede a newer deferred selection while that lock is held.
        if self.pending.is_some() || self.parametric.deferred.is_some() {
            return Err(Error::MutationBusy);
        }
        self.parametric_apply(None).await
    }

    pub(super) async fn parametric_apply_deferred(&mut self) -> Result<()> {
        if let Some(selection) = self.parametric.deferred.take() {
            let is_device = matches!(selection, Selection::Device(..));
            if let Err(error) = self.parametric_apply_selection(selection).await
                && (!is_device || self.parametric.failed_device.is_none())
            {
                return Err(error);
            }
        }
        self.parametric_supervise().await
    }

    async fn parametric_apply_selection(
        &mut self,
        selection: Selection,
    ) -> Result<ParametricApplication> {
        match selection {
            Selection::Plain(plan) => self.parametric_apply(plan.map(|plan| *plan)).await,
            Selection::ClearBound(_) | Selection::ClearDevice(_) | Selection::DeviceRequest(..) => {
                unreachable!("only accepted apply operations enter the deferred queue")
            }
            Selection::Device(target, plan) => {
                self.parametric.failed_device = None;
                self.parametric.device_selection_save_failed = false;
                self.parametric.device = Some(target.clone());
                self.parametric.possibly_attached = true;
                let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
                    if engine.apply_parametric_component(*plan)? {
                        Ok(ParametricApplication::Configured)
                    } else {
                        Err(Error::State("parametric audio processing is unavailable"))
                    }
                })
                .await;
                if result.is_ok() {
                    // A local preference failure must not undo confirmed
                    // native tuning or affect the current playback.
                    self.parametric.device_selection_save_failed = self
                        .session
                        .with_token(&self.token, |owner| {
                            RememberedHeadphone::record(
                                &self.store,
                                owner,
                                target.identity.target.brand,
                                target.identity.target.model,
                            )
                        })
                        .is_err();
                } else {
                    if let Err(cleanup) = self.parametric_reset().await {
                        let _ = self.stop(false).await;
                        return Err(cleanup);
                    }
                    // Only a successful, exact detach on the still-current
                    // owner lets a pending song open without this effect.
                    self.session.check_generation(&self.token)?;
                    self.parametric.failed_device = Some(target);
                }
                result
            }
            Selection::Bound(binding) => {
                let (session, token, checked) =
                    (self.session.clone(), self.token.clone(), binding.clone());
                self.parametric.possibly_attached = true;
                self.parametric.installing = Some(binding);
                let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
                    checked.with_current(&session, &token, |plan| {
                        if engine.apply_parametric_component(plan)? {
                            Ok(ParametricApplication::Configured)
                        } else {
                            Err(Error::State("parametric audio processing is unavailable"))
                        }
                    })
                })
                .await;
                if result.is_ok() {
                    self.parametric_record_applied_clear();
                    self.parametric.applied = self.parametric.installing.take();
                    self.parametric.failed_binding = None;
                }
                self.parametric_cleanup_after_failure(result).await
            }
        }
    }

    async fn parametric_apply(
        &mut self,
        plan: Option<ParametricPlan>,
    ) -> Result<ParametricApplication> {
        let Some(plan) = plan else {
            if let Err(error) = self.parametric_reset().await {
                let _ = self.stop(false).await;
                return Err(error);
            }
            self.session.check_generation(&self.token)?;
            return Ok(ParametricApplication::Cleared);
        };
        // Conservative bookkeeping: the native operation may have attached
        // before a generation change or an engine error was observed.
        let enabled = plan.enabled();
        self.parametric.possibly_attached |= enabled;
        let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
            if !enabled {
                engine.bypass_parametric_component()?;
                Ok(ParametricApplication::BypassConfigured)
            } else if engine.apply_parametric_component(plan)? {
                Ok(ParametricApplication::Configured)
            } else {
                Err(Error::State("parametric audio processing is unavailable"))
            }
        })
        .await;
        if enabled && result.is_ok() {
            self.parametric_record_applied_clear();
            self.parametric.release_unused_fence();
        }
        self.parametric_cleanup_after_failure(result).await
    }

    async fn parametric_cleanup_after_failure(
        &mut self,
        result: Result<ParametricApplication>,
    ) -> Result<ParametricApplication> {
        if result.is_err()
            && let Err(cleanup) = self.parametric_reset().await
        {
            // A failed removal cannot leave the old component audibly active.
            // Keep the reset flag so a later stop/rebind retries detachment.
            let _ = self.stop(false).await;
            return Err(cleanup);
        }
        result
    }

    pub(super) async fn parametric_supervise(&mut self) -> Result<()> {
        // The load worker already stopped old audio and loads paused; avoid
        // waiting on its engine lock. loaded() checks again before unpause.
        if self.pending.is_none()
            && self.parametric.applied.as_ref().is_some_and(|binding| {
                binding
                    .with_current(&self.session, &self.token, |_| Ok(()))
                    .is_err()
            })
            && let Err(error) = self.parametric_reset().await
        {
            let _ = self.stop(false).await;
            return Err(error);
        }
        Ok(())
    }

    /// Hold the local binding fences across a native resume. This closes the
    /// gap between an actor tick/request check and the actual engine command.
    pub(super) async fn parametric_transport<T: Send + 'static>(
        &self,
        paused: bool,
        cancel: Cancellation,
        operation: impl FnOnce(&mut dyn AudioEngine) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let binding = if paused {
            None
        } else {
            self.parametric.applied.clone()
        };
        let (session, token) = (self.session.clone(), self.token.clone());
        super::engine(self.engine.clone(), cancel, move |engine| {
            if let Some(binding) = binding {
                binding.with_current(&session, &token, |_| operation(engine))
            } else {
                operation(engine)
            }
        })
        .await
    }

    pub(super) async fn parametric_retry_reset(&mut self) -> Result<()> {
        if self.parametric.detach_required {
            let device = self.parametric.device.clone();
            self.parametric_reset().await?;
            self.parametric.failed_device = device;
        }
        Ok(())
    }

    pub(super) fn clear_parametric_failure_notice(&mut self) {
        self.parametric.failed_binding = None;
        self.parametric.failed_device = None;
    }

    pub(super) async fn poll_with_parametric_recovery(&mut self) -> Result<EngineSnapshot> {
        let result = super::engine(self.engine.clone(), self.guard(), |engine| engine.poll()).await;
        self.recover_parametric_poll(result).await
    }

    pub(super) async fn recover_parametric_poll(
        &mut self,
        result: Result<EngineSnapshot>,
    ) -> Result<EngineSnapshot> {
        match result {
            Err(error)
                if matches!(error.root(), Error::ParametricProcessorFailed)
                    && self.parametric.possibly_attached =>
            {
                let target = self
                    .parametric
                    .applied
                    .as_ref()
                    .map(|binding| binding.target());
                let device = self.parametric.device.clone();
                // A native callback may have emitted silence. Continue only
                // when removal was actually acknowledged; the actor's usual
                // error path stops playback if that cannot be established.
                self.parametric_reset().await?;
                self.parametric.failed_binding = target.map(|target| (self.token.clone(), target));
                self.parametric.failed_device = device;
                super::engine(self.engine.clone(), self.guard(), |engine| engine.poll()).await
            }
            result => result,
        }
    }

    fn parametric_record_applied_clear(&mut self) {
        if let Some(binding) = self.parametric.applied.take() {
            self.parametric.record_clear(&self.token, &binding);
        }
    }

    pub(super) async fn parametric_reset(&mut self) -> Result<()> {
        self.parametric.clear_deferred(&self.token);
        // Stop new observations/resume immediately, but keep the exact targets
        // and their OS fence until native removal actually succeeds.
        for binding in [
            self.parametric.applied.as_ref(),
            self.parametric.installing.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            binding.retire();
        }
        if self.parametric.possibly_attached {
            self.parametric.detach_required = true;
            super::engine(self.engine.clone(), Cancellation::default(), |engine| {
                engine.clear_parametric_component()
            })
            .await?;
            self.parametric.possibly_attached = false;
            self.parametric.detach_required = false;
        }
        self.parametric_record_applied_clear();
        self.parametric.device = None;
        self.parametric.device_selection_save_failed = false;
        if let Some(binding) = self.parametric.installing.take() {
            self.parametric.record_clear(&self.token, &binding);
        }
        self.parametric.release_unused_fence();
        Ok(())
    }
}
