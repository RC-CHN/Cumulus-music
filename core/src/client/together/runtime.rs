use super::*;
use crate::listen_together::{ListCommand, ListCommandKind, PlayCommand, PlayState};
use crate::playback::controller::{PlaybackSnapshot, PlaybackStatus};
use std::time::Duration;

pub(super) enum Signal {
    Player(Option<CommandKind>),
    Publish,
}
#[derive(Default)]
struct Intent {
    action: Option<CommandKind>,
    recover: bool,
}
impl Intent {
    fn receive(&mut self, signal: Signal) {
        match signal {
            Signal::Player(Some(kind)) => self.action = Some(kind),
            Signal::Player(None) => {}
            Signal::Publish => {
                self.recover = true;
                self.action = Some(CommandKind::Progress);
            }
        }
    }
}

pub(super) struct Context {
    pub session: Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub controller: Arc<PlaybackController>,
    pub token: SessionToken,
    pub room_id: String,
    pub gate: Arc<AsyncMutex<()>>,
    pub error: Arc<Mutex<Option<String>>>,
    pub messages: mpsc::Receiver<crate::listen_together::transport::TransportEvent>,
    pub initial_sync: bool,
    pub connection:
        Option<tokio::sync::watch::Receiver<crate::listen_together::transport::ConnectionView>>,
    pub _im_cleanup: Option<crate::listen_together::transport::TransportCancellation>,
    pub im: Option<Arc<crate::listen_together::transport::ImTransport>>,
    pub cleanup_uncertain: Arc<Mutex<bool>>,
}
fn key(s: &PlaybackSnapshot) -> (u64, Option<String>, Option<String>, PlaybackStatus) {
    (
        s.queue_revision,
        s.selected.as_ref().map(|e| e.entry_id.clone()),
        s.play_id.clone(),
        s.state,
    )
}
fn playing(s: &PlaybackSnapshot) -> bool {
    matches!(s.state, PlaybackStatus::Playing | PlaybackStatus::Buffering)
}
impl Context {
    pub async fn run(mut self, mut signals: mpsc::UnboundedReceiver<Signal>) {
        let mut receiver = self.controller.subscribe();
        let mut last = if self.initial_sync {
            None
        } else {
            Some(self.controller.snapshot())
        };
        let mut intent = Intent::default();
        let mut work = self.initial_sync;
        let mut messages_open = true;
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if self.session.with_token(&self.token, |_| Ok(())).is_err() {
                break;
            }
            if work {
                let _gate = self.gate.lock().await;
                if self.error.lock().map(|v| v.is_none()).unwrap_or(false) {
                    match self
                        .synchronize(&mut last, std::mem::take(&mut intent))
                        .await
                    {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(Error::StaleResponse) => break,
                        Err(error) => {
                            if let Ok(mut notice) = self.error.lock() {
                                *notice = Some(message(error));
                            }
                        }
                    }
                }
                work = false;
            }
            tokio::select! {
                biased;
                // A continuous native clock (especially with a latched sync
                // error) must not starve authoritative room-loss cleanup.
                _=timer.tick()=>{
                    let _gate=self.gate.lock().await;
                    let service=match TogetherService::new(self.session.clone()){Ok(v)=>v,Err(_)=>break};
                    let state=match service.state_for(&self.token){Ok(v)=>v,Err(_)=>break};
                    if state.room.as_ref().map(|r|r.room_id.as_str())!=Some(self.room_id.as_str()){break;}
                    if let Err(error)=service.tick_for(&self.token,now()).await
                        && let Ok(mut notice)=self.error.lock(){*notice=Some(message(error));}
                }
                event=self.messages.recv(), if messages_open=>{
                    if let Some(event)=event {
                        let _gate=self.gate.lock().await;
                        if let Err(error)=self.on_transport(event,&mut last).await
                            && let Ok(mut notice)=self.error.lock(){*notice=Some(message(error));}
                    } else {messages_open=false;}
                }
                value=signals.recv()=>{
                    let Some(value)=value else {break;};intent.receive(value);work=true;
                    while let Ok(next)=signals.try_recv(){intent.receive(next);}
                }
                changed=receiver.changed()=>{
                    if changed.is_err(){break;}
                    let snapshot=receiver.borrow_and_update().clone();
                    if last.as_ref().is_none_or(|old|key(old)!=key(&snapshot)) {
                        // Give the semantic action notification a chance to
                        // arrive after the native command reply. Position-only
                        // engine ticks do not trigger HTTP reports.
                        tokio::time::sleep(Duration::from_millis(75)).await;
                        while let Ok(next)=signals.try_recv(){intent.receive(next);}
                        work=true;
                    }
                }
            }
        }
        if let Err(error) = self.finish_runtime().await
            && !matches!(error.root(), Error::StaleResponse)
            && let Ok(mut notice) = self.error.lock()
        {
            *notice = Some(message(error));
        }
    }
    async fn on_transport(
        &self,
        event: crate::listen_together::transport::TransportEvent,
        last: &mut Option<PlaybackSnapshot>,
    ) -> Result<()> {
        use crate::listen_together::{native_im::ImEvent, transport::TransportEvent};
        let service = TogetherService::new(self.session.clone())?;
        let state = service.state_for(&self.token)?;
        let Some(room) = state.room.as_ref().filter(|r| r.room_id == self.room_id) else {
            return Ok(());
        };
        match event {
            TransportEvent::Entered { .. } => {
                service.refresh_for(&self.token, now()).await?;
            }
            TransportEvent::Message(ImEvent::Exit { room: chat, .. })
                if chat.to_string() == room.chat_room_id =>
            {
                service.refresh_for(&self.token, now()).await?;
            }
            TransportEvent::Message(
                ImEvent::Message { room: chat, .. } | ImEvent::Notification { room: chat, .. },
            ) if chat.to_string() == room.chat_room_id => {
                service.message_received_for(&self.token, &self.room_id, now())?;
            }
            TransportEvent::Message(ImEvent::Business { messages })
                if messages
                    .iter()
                    .any(|message| message.belongs_to(&self.room_id)) =>
            {
                // The transport already filters these, but the room may have
                // changed while the event waited behind an account operation.
                // This is M/75's heartbeatReCount, before business dispatch.
                service.message_received_for(&self.token, &self.room_id, now())?;
                for message in messages.into_iter().filter(|v| v.belongs_to(&self.room_id)) {
                    use crate::listen_together::MessageKind;
                    let role = service.state_for(&self.token)?.role;
                    if message.kind == MessageKind::End {
                        if service.receive_end_for(&self.token, &self.room_id, now())? {
                            let cleanup = self.close_im().await;
                            let ending = service
                                .resolve_end_notice_for(&self.token, &self.room_id, now())
                                .await;
                            cleanup?;
                            ending?;
                        }
                        if role != Some(RoomRole::Waiting) {
                            break;
                        }
                        continue;
                    }
                    if service.state_for(&self.token)?.end_notice.is_some() {
                        continue;
                    }
                    match message.kind {
                        MessageKind::Play => {
                            if let Some(snapshot) = self
                                .incoming()
                                .receive(PlayCommand::from_official(&message.content)?)
                                .await?
                            {
                                *last = Some(snapshot);
                            }
                        }
                        MessageKind::MemberJoined if role == Some(RoomRole::Waiting) => {
                            let refreshed = service.refresh_for(&self.token, now()).await?;
                            if refreshed.role != Some(RoomRole::Waiting)
                                && let Some(snapshot) =
                                    self.incoming().refresh_shared(None, false).await?
                            {
                                *last = Some(snapshot);
                            }
                        }
                        MessageKind::Playlist if role != Some(RoomRole::Waiting) => {
                            let version = crate::listen_together::Version::parse(
                                message.content.get("version").unwrap_or(&json!([])),
                            )?;
                            if let Some(snapshot) =
                                self.incoming().refresh_shared(Some(&version), true).await?
                            {
                                *last = Some(snapshot);
                            }
                        }
                        MessageKind::DigitalChanged | MessageKind::PrivilegeChanged
                            if role != Some(RoomRole::Waiting) =>
                        {
                            if let Some(snapshot) =
                                self.incoming().refresh_shared(None, false).await?
                            {
                                *last = Some(snapshot);
                            }
                        }
                        MessageKind::Heartbeat => {
                            let owner = self
                                .token
                                .uid
                                .as_deref()
                                .ok_or(Error::State("no account"))?;
                            if message.requests_progress_for(owner)? {
                                self.report_requested_progress().await?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn incoming(&self) -> incoming::Receiver<'_> {
        incoming::Receiver {
            session: &self.session,
            store: self.store.clone(),
            controller: &self.controller,
            token: &self.token,
            room: &self.room_id,
        }
    }
    async fn synchronize(
        &self,
        last: &mut Option<PlaybackSnapshot>,
        intent: Intent,
    ) -> Result<bool> {
        let service = TogetherService::new(self.session.clone())?;
        let mut remote = service.state_for(&self.token)?;
        if remote.room.as_ref().map(|r| r.room_id.as_str()) != Some(self.room_id.as_str()) {
            return Ok(true);
        }
        if remote.end_notice.is_some() {
            return Err(Error::State("together room is ending"));
        }
        if remote
            .shared
            .as_ref()
            .is_some_and(|v| v.phase != crate::listen_together::IncomingPhase::Applied)
        {
            if intent.recover {
                *last = Some(
                    self.incoming()
                        .refresh_shared(None, false)
                        .await?
                        .unwrap_or_else(|| self.controller.snapshot()),
                );
                return Ok(false);
            }
            return Err(Error::State("shared queue requires recovery"));
        }
        if remote.incoming.as_ref().is_some_and(|v| {
            matches!(
                v.phase,
                crate::listen_together::IncomingPhase::Pending
                    | crate::listen_together::IncomingPhase::Failed
            )
        }) {
            if intent.recover {
                *last = Some(
                    self.incoming()
                        .retry()
                        .await?
                        .unwrap_or_else(|| self.controller.snapshot()),
                );
                return Ok(false);
            }
            return Err(Error::State("remote playback requires recovery"));
        }
        let action = intent.action;
        if remote.role != Some(RoomRole::Waiting) {
            if intent.recover {
                *last = Some(
                    self.incoming()
                        .refresh_shared(None, false)
                        .await?
                        .unwrap_or_else(|| self.controller.snapshot()),
                );
                return Ok(false);
            }
            if !remote
                .shared
                .as_ref()
                .is_some_and(|v| v.phase == crate::listen_together::IncomingPhase::Applied)
            {
                return Err(Error::State("shared queue requires recovery"));
            }
            if self.connection.as_ref().is_none_or(|v| {
                v.borrow().state != crate::listen_together::transport::ConnectionState::Connected
            }) {
                return Err(Error::State("realtime together connection is not ready"));
            }
        }
        if service
            .operations_for(&self.token)?
            .iter()
            .any(|op| op.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let (mut snapshot, queue) = self.controller.queue_with_state(&self.token).await?;
        if matches!(
            snapshot.state,
            PlaybackStatus::Preparing | PlaybackStatus::Loading
        ) {
            return Ok(false);
        }
        let shared =
            selection::shared_selection(&self.session, &self.token, self.store.clone(), &queue)?;
        self.session.with_token(&self.token, |uid| {
            self.store.put(
                uid,
                "listen-together-cards-v1",
                "current",
                &serde_json::to_value(&shared.cards)?,
            )
        })?;
        if snapshot.mode == PlaybackMode::Sequential {
            snapshot = self
                .controller
                .command(&self.token, PlaybackCommand::Mode(PlaybackMode::OrderLoop))
                .await?;
        }
        if shared.display.is_empty() {
            if remote
                .snapshot
                .playlist
                .as_ref()
                .is_some_and(|p| !p.display.is_empty())
                && queue.items.is_empty()
            {
                let ended = service
                    .end_for(&self.token, &self.room_id, None, now())
                    .await?;
                if ended.phase != TogetherPhase::Confirmed {
                    return Err(Error::MutationUncertain);
                }
                return Ok(true);
            }
            if !queue.items.is_empty() {
                return Err(Error::State("current resources cannot join together"));
            }
            *last = Some(snapshot);
            return Ok(false);
        }
        let Some(target) = shared.current else {
            return Err(Error::State("current resource cannot join together"));
        };
        let changed = remote.snapshot.playlist.as_ref().is_none_or(|p| {
            p.display != shared.display
                || p.mode != shared.mode
                || (shared.mode == SharedMode::Random && p.random.as_ref() != Some(&shared.random))
        });
        if changed {
            let cmd = ListCommand {
                command_type: ListCommandKind::Replace,
                version: remote
                    .snapshot
                    .playlist
                    .as_ref()
                    .map(|p| p.version.clone())
                    .unwrap_or_default(),
                play_mode: shared.mode,
                anchor_song_id: String::new(),
                anchor_position: -1,
                display_list: shared.display,
                random_list: shared.random,
            };
            let result = service
                .queue_for(&self.token, &self.room_id, cmd, now())
                .await?;
            if result.phase != TogetherPhase::Confirmed {
                return Err(Error::MutationUncertain);
            }
            remote = service.refresh_for(&self.token, now()).await?;
            if remote.role != Some(RoomRole::Waiting) {
                let Some(applied) = self.incoming().refresh_shared(None, true).await? else {
                    return Ok(false);
                };
                snapshot = applied;
                remote = service.state_for(&self.token)?;
            }
        }
        // A newer native selection wins while a queue request was in flight.
        if key(&self.controller.snapshot()) != key(&snapshot) {
            return Ok(false);
        }
        let previous = remote.playback.latest.as_ref().map(|c| &c.command);
        let target_changed = previous.is_none_or(|p| p.target_song_id != target);
        // Explicit resynchronization after an uncertain write may find a
        // different local song. PROGRESS cannot select it on the peer.
        let kind = action
            .filter(|kind| *kind != CommandKind::Progress || !target_changed)
            .or_else(|| {
                if target_changed {
                    Some(CommandKind::Goto)
                } else if previous
                    .is_some_and(|p| (p.play_status == PlayState::Play) != playing(&snapshot))
                {
                    Some(if playing(&snapshot) {
                        CommandKind::Play
                    } else {
                        CommandKind::Pause
                    })
                } else if changed {
                    Some(CommandKind::Progress)
                } else {
                    None
                }
            });
        if let Some(kind) = kind {
            let former = previous
                .map(|p| p.target_song_id.as_str())
                .unwrap_or(&target);
            let seconds = snapshot
                .position
                .unwrap_or_else(|| previous.map_or(0.0, |p| p.progress as f64 / 1000.0));
            // TimelineSnapshot/position already use full-song time for trial
            // media; adding its start again would double-offset the command.
            let command = PlayCommand::at_seconds(
                kind,
                seconds,
                if playing(&snapshot) {
                    PlayState::Play
                } else {
                    PlayState::Pause
                },
                former,
                &target,
            )?;
            let result = if action == Some(CommandKind::Progress) && kind == CommandKind::Progress {
                service
                    .seek_for(
                        &self.token,
                        &self.room_id,
                        command,
                        playing(&snapshot),
                        now(),
                    )
                    .await?
            } else {
                service
                    .play_for(
                        &self.token,
                        &self.room_id,
                        command,
                        playing(&snapshot),
                        now(),
                    )
                    .await?
            };
            if let Some(result) = result
                && result.phase != TogetherPhase::Confirmed
            {
                return Err(Error::MutationUncertain);
            }
        }
        *last = Some(snapshot);
        Ok(false)
    }
}
fn message(error: Error) -> String {
    match error.root() {
        Error::State(
            "current resources cannot join together" | "current resource cannot join together",
        ) => "当前内容暂不能加入一起听，请改播普通歌曲后继续同步".into(),
        Error::State("realtime together connection is not ready") => {
            "好友实时连接尚未就绪，已暂停房间播放同步".into()
        }
        Error::State("remote playback requires recovery") => {
            "远端播放指令尚未完成，请重新同步以核对并恢复".into()
        }
        Error::State("remote track is not in the native queue") => {
            "远端歌曲尚未加入本机播放列表，播放同步未完成".into()
        }
        Error::State("shared queue requires recovery") => {
            "共享队列或共同播放权限尚未同步完成，请重新同步".into()
        }
        _ => ClientError::from(error).message.into(),
    }
}
