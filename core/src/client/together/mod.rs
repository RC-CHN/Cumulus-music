mod connection;
mod dto;
mod incoming;
mod lifecycle;
mod progress;
mod queue_policy;
mod runtime;
mod selection;
mod shared;
mod view;
use super::{
    ClientError, MusicClient,
    dto::{AccountData, PlayerAction},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    listen_together::{CommandKind, RoomRole, SharedMode, TogetherPhase, TogetherService},
    playback::{
        PlaybackMode,
        controller::{PlaybackCommand, PlaybackController},
    },
    storage::StateStore,
};
pub use dto::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

const BINDING: &str = "listen-together-player-v1";
#[derive(Clone, Serialize, Deserialize)]
struct Binding {
    room_id: String,
    previous_speed: f64,
    previous_mode: PlaybackMode,
}
struct Runtime {
    token: SessionToken,
    room_id: String,
    sender: mpsc::UnboundedSender<runtime::Signal>,
    worker: tokio::task::JoinHandle<()>,
    im: Option<Arc<crate::listen_together::transport::ImTransport>>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
#[derive(Default)]
pub(super) struct TogetherHost {
    pub(super) gate: Arc<AsyncMutex<()>>,
    runtime: Mutex<Option<Runtime>>,
    error: Arc<Mutex<Option<String>>>,
    factory: Option<Arc<dyn crate::listen_together::transport::ImFactory>>,
    cleanup_uncertain: Arc<Mutex<bool>>,
}
pub(super) fn signal(action: &PlayerAction) -> Option<CommandKind> {
    match action {
        PlayerAction::Seek { .. } => Some(CommandKind::Progress),
        PlayerAction::Next => Some(CommandKind::Next),
        PlayerAction::Previous => Some(CommandKind::Previous),
        PlayerAction::Select { .. } => Some(CommandKind::Goto),
        _ => None,
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
impl MusicClient {
    pub fn has_together_cleanup(&self) -> Result<bool> {
        let token = self.session.token()?;
        if token.uid.is_none() {
            return Ok(false);
        }
        self.together_binding(&token).map(|b| b.is_some())
    }
    fn together_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        let mut runtime = self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if runtime.as_ref().is_some_and(|r| r.token != token) {
            runtime.take();
            *self
                .together
                .error
                .lock()
                .map_err(|_| Error::LockPoisoned)? = None;
        }
        Ok(token)
    }
    pub(super) fn together_validate_action(
        &self,
        token: &SessionToken,
        action: &PlayerAction,
    ) -> Result<()> {
        let attached = self.together_attached(token, None)?;
        if attached
            && matches!(action, PlayerAction::Mode { mode } if !crate::playback::mode::ModeFamily::Together.supports(*mode))
        {
            return Err(Error::State(
                "together does not support sequential playback",
            ));
        }
        if attached && matches!(action,PlayerAction::Speed {value} if *value!=1.0) {
            return Err(Error::State("together requires normal playback speed"));
        }
        Ok(())
    }
    pub(super) fn together_attached(
        &self,
        token: &SessionToken,
        room: Option<&str>,
    ) -> Result<bool> {
        Ok(self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .is_some_and(|r| {
                r.token == *token
                    && !r.worker.is_finished()
                    && room.is_none_or(|id| id == r.room_id)
            }))
    }
    pub(super) fn together_signal(&self, token: &SessionToken, kind: Option<CommandKind>) {
        self.together_notify(token, runtime::Signal::Player(kind));
    }
    fn together_notify(&self, token: &SessionToken, signal: runtime::Signal) {
        if let Ok(runtime) = self.together.runtime.lock()
            && let Some(r) = runtime.as_ref()
            && r.token == *token
        {
            let _ = r.sender.send(signal);
        }
    }
    fn together_binding(&self, token: &SessionToken) -> Result<Option<Binding>> {
        self.session.with_token(token, |uid| {
            self.registry
                .store()
                .get(uid, BINDING, "current")?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    pub async fn together_view(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<TogetherView>> {
        let token = self.together_token(epoch)?;
        let _gate = if refresh {
            Some(self.together.gate.lock().await)
        } else {
            None
        };
        let service = TogetherService::new(self.session.clone())?;
        let state = if refresh {
            service.refresh_for(&token, now()).await?
        } else {
            service.state_for(&token)?
        };
        view::project(self, &token, epoch, state, refresh).await
    }
    pub async fn together_command(
        &self,
        epoch: &str,
        command: TogetherCommand,
    ) -> Result<AccountData<TogetherView>> {
        let token = self.together_token(epoch)?;
        let _gate = self.together.gate.lock().await;
        if matches!(
            &command,
            TogetherCommand::Create | TogetherCommand::Attach { .. }
        ) && self.login_pending()?
        {
            return Err(Error::State("finish login before entering together"));
        }
        let service = TogetherService::new(self.session.clone())?;
        let removing = matches!(&command, TogetherCommand::Remove { .. });
        if matches!(
            &command,
            TogetherCommand::Create | TogetherCommand::Attach { .. }
        ) {
            let queue = self.player.controller().await?.queue(&token).await?;
            if queue
                .intelligence
                .as_ref()
                .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
            {
                return Err(Error::State("leave intelligence before entering together"));
            }
            if self
                .player
                .fm()
                .await?
                .active_source_for(&token, &queue)?
                .is_some()
            {
                return Err(Error::State("leave FM before entering together"));
            }
        }
        match command {
            TogetherCommand::Create => {
                if self.together_binding(&token)?.is_some() {
                    return Err(Error::State(
                        "restore previous together playback settings first",
                    ));
                }
                let op = service.create_for(&token, "cumulus_desktop", now()).await?;
                if op.phase != TogetherPhase::Confirmed {
                    return Err(Error::MutationUncertain);
                }
                let room = op.created_room.ok_or(Error::State("room already exists"))?;
                self.together_attach(&token, &room.room_id, true).await?;
            }
            TogetherCommand::Attach { room_id } => {
                let state = service.refresh_for(&token, now()).await?;
                if state.room.as_ref().map(|r| &r.room_id) != Some(&room_id) {
                    return Err(Error::State("together room changed"));
                }
                let recovering = self.together.factory.is_some()
                    && (state.shared.as_ref().is_some_and(|v| {
                        matches!(
                            v.phase,
                            crate::listen_together::IncomingPhase::Pending
                                | crate::listen_together::IncomingPhase::Failed
                        )
                    }) || state.incoming.as_ref().is_some_and(|v| {
                        matches!(
                            v.phase,
                            crate::listen_together::IncomingPhase::Pending
                                | crate::listen_together::IncomingPhase::Failed
                        )
                    }))
                    && self
                        .together_binding(&token)?
                        .is_some_and(|b| b.room_id == room_id);
                let joining =
                    state.role != Some(RoomRole::Waiting) && self.together.factory.is_some();
                if state.role != Some(RoomRole::Waiting) && !recovering && !joining {
                    return Err(Error::State("realtime together connection is not ready"));
                }
                self.together_attach(&token, &room_id, !recovering && !joining)
                    .await?;
                if recovering || joining {
                    self.together_notify(&token, runtime::Signal::Publish);
                }
            }
            TogetherCommand::Reconnect { room_id } => {
                if self.together.factory.is_none()
                    || !self.together_attached(&token, Some(&room_id))?
                {
                    return Err(Error::State("together message connection is not attached"));
                }
                self.together_attach(&token, &room_id, false).await?;
            }
            TogetherCommand::Publish { room_id } => {
                if !self.together_attached(&token, Some(&room_id))? {
                    return Err(Error::State("together player is not attached"));
                }
                if service
                    .operations_for(&token)?
                    .iter()
                    .any(|op| op.phase.pending())
                {
                    return Err(Error::MutationUncertain);
                }
                *self
                    .together
                    .error
                    .lock()
                    .map_err(|_| Error::LockPoisoned)? = None;
                self.together_notify(&token, runtime::Signal::Publish);
            }
            TogetherCommand::Reconcile { operation_id } => {
                service.reconcile_for(&token, &operation_id, now()).await?;
            }
            TogetherCommand::Select { room_id, song_id }
            | TogetherCommand::Remove { room_id, song_id } => {
                if !self.together_attached(&token, Some(&room_id))? {
                    return Err(Error::State("together player is not attached"));
                }
                let controller = self.player.controller().await?;
                let queue = controller.queue(&token).await?;
                let selected = selection::shared_selection(
                    &self.session,
                    &token,
                    self.registry.store(),
                    &queue,
                )?;
                let entries = queue
                    .items
                    .iter()
                    .filter(|i| selected.entries.get(&i.entry_id) == Some(&song_id))
                    .map(|i| i.entry_id.clone())
                    .collect::<Vec<_>>();
                if entries.is_empty() {
                    return Err(Error::QueueChanged);
                }
                let action = if removing {
                    PlayerAction::Remove { entry_ids: entries }
                } else {
                    PlayerAction::Select {
                        entry_id: entries[0].clone(),
                    }
                };
                self.player_action(epoch, action).await?;
            }
            TogetherCommand::End { room_id } => {
                let op = service.end_for(&token, &room_id, None, now()).await?;
                if op.phase != TogetherPhase::Confirmed {
                    return Err(Error::MutationUncertain);
                }
                self.together_stop().await?;
                self.together_restore_local(&token).await?;
            }
            TogetherCommand::RestoreLocal => {
                let state = service.refresh_for(&token, now()).await?;
                let binding = self.together_binding(&token)?;
                if state
                    .room
                    .as_ref()
                    .is_some_and(|r| binding.as_ref().is_some_and(|b| b.room_id == r.room_id))
                {
                    return Err(Error::State(
                        "leave together before restoring local settings",
                    ));
                }
                self.together_stop().await?;
                self.together_restore_local(&token).await?;
            }
        }
        let state = service.state_for(&token)?;
        view::project(self, &token, epoch, state, true).await
    }
    async fn together_attach(
        &self,
        token: &SessionToken,
        room: &str,
        initial_sync: bool,
    ) -> Result<()> {
        let service = TogetherService::new(self.session.clone())?;
        if service.state_for(token)?.end_notice.is_some() {
            return Err(Error::State("together room is ending"));
        }
        if service
            .operations_for(token)?
            .iter()
            .any(|op| op.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        self.together_stop().await?;
        let controller = self.player.controller().await?.clone();
        let current = controller.command(token, PlaybackCommand::Snapshot).await?;
        let binding = if let Some(saved) = self.together_binding(token)? {
            if saved.room_id != room {
                return Err(Error::State(
                    "restore previous together playback settings first",
                ));
            }
            saved
        } else {
            Binding {
                room_id: room.into(),
                previous_speed: current.speed,
                previous_mode: current.mode,
            }
        };
        self.session.with_token(token, |uid| {
            self.registry
                .store()
                .put(uid, BINDING, "current", &serde_json::to_value(&binding)?)?;
            self.registry
                .store()
                .delete(uid, lifecycle::NOTICE, "current")
        })?;
        controller
            .command(token, PlaybackCommand::Speed(1.0))
            .await?;
        if current.mode == PlaybackMode::Sequential {
            controller
                .command(token, PlaybackCommand::Mode(PlaybackMode::OrderLoop))
                .await?;
        }
        *self
            .together
            .error
            .lock()
            .map_err(|_| Error::LockPoisoned)? = None;
        let (sender, receiver) = mpsc::unbounded_channel();
        let state = service.state_for(token)?;
        let active_room = state
            .room
            .filter(|r| r.room_id == room)
            .ok_or(Error::State(
                "together room changed before message connection",
            ))?;
        let (im, events) = if let Some(factory) = &self.together.factory {
            let (im, receiver) = crate::listen_together::transport::ImTransport::start(
                factory.clone(),
                self.session.clone(),
                token.clone(),
                active_room,
            );
            (Some(Arc::new(im)), receiver)
        } else {
            (None, mpsc::channel(1).1)
        };
        let context = runtime::Context {
            session: self.session.clone(),
            store: self.registry.store(),
            controller,
            token: token.clone(),
            room_id: room.into(),
            gate: self.together.gate.clone(),
            error: self.together.error.clone(),
            messages: events,
            initial_sync,
            connection: im.as_ref().map(|im| im.subscribe()),
            _im_cleanup: im.as_ref().map(|im| im.cancel_on_drop()),
            im: im.clone(),
            cleanup_uncertain: self.together.cleanup_uncertain.clone(),
        };
        let worker = tokio::spawn(context.run(receiver));
        *self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)? = Some(Runtime {
            token: token.clone(),
            room_id: room.into(),
            sender,
            worker,
            im,
        });
        Ok(())
    }
    async fn together_stop(&self) -> Result<()> {
        if *self
            .together
            .cleanup_uncertain
            .lock()
            .map_err(|_| Error::LockPoisoned)?
        {
            return Err(Error::State("IM cleanup unresolved; restart the process"));
        }
        let runtime = self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .take();
        if let Some(mut runtime) = runtime {
            runtime.worker.abort();
            if let Some(im) = runtime.im.take()
                && let Err(error) = im.close().await
            {
                *self
                    .together
                    .cleanup_uncertain
                    .lock()
                    .map_err(|_| Error::LockPoisoned)? = true;
                return Err(error);
            }
        }
        *self
            .together
            .error
            .lock()
            .map_err(|_| Error::LockPoisoned)? = None;
        Ok(())
    }
    async fn together_restore_local(&self, token: &SessionToken) -> Result<()> {
        if self.together_binding(token)?.is_none() {
            return Ok(());
        }
        lifecycle::restore_local(
            &self.session,
            self.registry.store(),
            self.player.controller().await?.as_ref(),
            token,
            None,
        )
        .await?;
        Ok(())
    }
    /// Native host close hook. Only rooms explicitly attached by this client
    /// are eligible; an observed room from another device is never auto-ended.
    pub async fn close_together(&self) -> Result<()> {
        let token = self.session.token()?;
        if token.uid.is_none() {
            return Ok(());
        }
        let _gate = self.together.gate.lock().await;
        let Some(binding) = self.together_binding(&token)? else {
            return Ok(());
        };
        let service = TogetherService::new(self.session.clone())?;
        for op in service.operations_for(&token)? {
            if op.phase.pending() {
                service.reconcile_for(&token, &op.id, now()).await?;
            }
        }
        let state = service.refresh_for(&token, now()).await?;
        if state
            .room
            .as_ref()
            .is_some_and(|r| r.room_id == binding.room_id)
        {
            let result = service
                .end_for(&token, &binding.room_id, None, now())
                .await?;
            if result.phase != TogetherPhase::Confirmed {
                return Err(Error::MutationUncertain);
            }
        }
        self.together_stop().await?;
        self.together_restore_local(&token).await
    }
}
