use super::{Message, PlaybackController, state::Actor};
use crate::{
    Error, Result, account::SessionToken, effects::parametric::ParametricPlan,
    playback::engine::Cancellation,
};
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
    plan: Option<ParametricPlan>,
    reply: oneshot::Sender<Result<ParametricApplication>>,
}

#[derive(Default)]
pub(super) struct State {
    // Outer Option distinguishes an explicit deferred Clear from no request.
    deferred: Option<Option<ParametricPlan>>,
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
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Message::Parametric(Box::new(Request {
                token: token.clone(),
                plan,
                reply,
            })))
            .map_err(|_| Error::State("player is closed"))?;
        result.await.map_err(|_| Error::Cancelled)?
    }
}

impl Actor {
    pub(super) async fn parametric_request(&mut self, request: Request) {
        if request.reply.is_closed() {
            return;
        }
        let result = self.parametric_change(&request.token, request.plan).await;
        let _ = request.reply.send(result);
    }

    async fn parametric_change(
        &mut self,
        token: &SessionToken,
        plan: Option<ParametricPlan>,
    ) -> Result<ParametricApplication> {
        self.rebind().await?;
        if !self.bound || token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.check_generation(token)?;
        if self.pending.is_some() {
            // Do not wait on the engine mutex held by a decoder. The latest
            // request will run in loaded(), before its first unpause.
            self.parametric.deferred = Some(plan);
            return Ok(ParametricApplication::DeferredUntilLoaded);
        }
        self.parametric_apply(plan).await
    }

    pub(super) async fn parametric_apply_deferred(&mut self) -> Result<()> {
        if let Some(plan) = self.parametric.deferred.take() {
            self.parametric_apply(plan).await?;
        }
        Ok(())
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

    pub(super) async fn parametric_retry_reset(&mut self) -> Result<()> {
        if self.parametric.detach_required {
            self.parametric_reset().await?;
        }
        Ok(())
    }

    pub(super) async fn parametric_reset(&mut self) -> Result<()> {
        self.parametric.deferred = None;
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
