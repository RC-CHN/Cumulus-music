use super::{Message, PlaybackController, state::Actor};
use crate::{
    Error, Result,
    account::SessionToken,
    effects::{
        authorization::{ActiveParametricObservation, BoundParametric},
        parametric::ParametricPlan,
    },
    playback::engine::{AudioEngine, Cancellation},
};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Configuration acknowledgement, not a claim of observed PCM processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParametricApplication {
    Cleared,
    Configured,
    BypassConfigured,
    DeferredUntilLoaded,
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
}

#[derive(Default)]
pub(super) struct State {
    deferred: Option<Selection>,
    applied: Option<Arc<BoundParametric>>,
    possibly_attached: bool,
    detach_required: bool,
}

impl PlaybackController {
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
        if let Selection::Bound(binding) = &selection {
            binding.with_current(&self.session, token, |_| Ok(()))?;
        }
        if self.pending.is_some() {
            // Do not wait on the engine mutex held by a decoder. The latest
            // request will run in loaded(), before its first unpause.
            self.parametric.deferred = Some(selection);
            return Ok(ParametricApplication::DeferredUntilLoaded);
        }
        self.parametric_apply_selection(selection).await
    }

    pub(super) async fn parametric_apply_deferred(&mut self) -> Result<()> {
        if let Some(selection) = self.parametric.deferred.take() {
            self.parametric_apply_selection(selection).await?;
        }
        self.parametric_supervise().await
    }

    async fn parametric_apply_selection(
        &mut self,
        selection: Selection,
    ) -> Result<ParametricApplication> {
        match selection {
            Selection::Plain(plan) => self.parametric_apply(plan.map(|plan| *plan)).await,
            Selection::Bound(binding) => {
                let (session, token, checked) =
                    (self.session.clone(), self.token.clone(), binding.clone());
                self.parametric.possibly_attached = true;
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
                    self.parametric_retire_applied();
                    self.parametric.applied = Some(binding);
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
            self.parametric_retire_applied();
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
            self.parametric_reset().await?;
        }
        Ok(())
    }

    fn parametric_retire_applied(&mut self) {
        if let Some(binding) = self.parametric.applied.take() {
            binding.retire();
        }
    }

    pub(super) async fn parametric_reset(&mut self) -> Result<()> {
        self.parametric.deferred = None;
        self.parametric_retire_applied();
        if self.parametric.possibly_attached {
            self.parametric.detach_required = true;
            super::engine(self.engine.clone(), Cancellation::default(), |engine| {
                engine.clear_parametric_component()
            })
            .await?;
            self.parametric.possibly_attached = false;
            self.parametric.detach_required = false;
        }
        Ok(())
    }
}
