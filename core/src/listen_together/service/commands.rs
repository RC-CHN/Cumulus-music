use super::*;
use crate::listen_together::model;
impl TogetherService {
    pub async fn create_for(
        &self,
        token: &SessionToken,
        refer: &str,
        now: u64,
    ) -> Result<TogetherOperation> {
        model::opaque(refer)?;
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_resolved(token)?;
        if self.status(token).await?.is_some() {
            return Err(Error::State("account already has a together room"));
        }
        self.send(
            token,
            WriteIntent::Create {
                refer: refer.into(),
            },
            now,
        )
        .await
    }
    pub async fn accept_for(
        &self,
        token: &SessionToken,
        room: &str,
        inviter: &str,
        refer: &str,
        now: u64,
    ) -> Result<TogetherOperation> {
        model::opaque(room)?;
        model::opaque(refer)?;
        crate::protocol::api::identifier(&serde_json::json!(inviter))?;
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_resolved(token)?;
        if self.status(token).await?.is_some() {
            return Err(Error::State("leave the existing room before accepting"));
        }
        self.send(
            token,
            WriteIntent::Accept {
                room_id: room.into(),
                inviter: inviter.into(),
                refer: refer.into(),
            },
            now,
        )
        .await
    }
    /// Explicit exit may supersede an unresolved queue/play write, but first
    /// verifies the current room. It never exits a different observed room.
    pub async fn end_for(
        &self,
        token: &SessionToken,
        room: &str,
        count: Option<u64>,
        now: u64,
    ) -> Result<TogetherOperation> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        if self
            .operations_for(token)?
            .iter()
            .any(|op| op.phase.pending() && matches!(&op.intent, WriteIntent::End { .. }))
        {
            return Err(Error::MutationUncertain);
        }
        let found = self
            .status(token)
            .await?
            .ok_or(Error::State("account no longer has a together room"))?;
        if found.room_id != room {
            return Err(Error::State("refusing to exit a different together room"));
        }
        self.send(
            token,
            WriteIntent::End {
                room_id: room.into(),
                count,
            },
            now,
        )
        .await
    }
    pub async fn queue_for(
        &self,
        token: &SessionToken,
        room: &str,
        mut command: ListCommand,
        now: u64,
    ) -> Result<TogetherOperation> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_outgoing_ready(token)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        let before = self.snapshot(token, room).await?;
        let versions = before
            .playlist
            .as_ref()
            .map(|p| p.version.as_slice())
            .unwrap_or(&[]);
        // The caller supplies the version it displayed. No silent overwrite of
        // an unseen queue changed by another device/participant.
        if !versions_equal(&command.version, versions) {
            return Err(Error::QueueChanged);
        }
        command.version = Version::increment(versions, &owner)?;
        command.validate()?;
        let mut expected = before
            .playlist
            .as_ref()
            .map(|p| p.display.clone())
            .unwrap_or_default();
        match command.command_type {
            ListCommandKind::Replace | ListCommandKind::PlaymodeChange => {
                expected = command.display_list.clone()
            }
            ListCommandKind::Delete => expected.retain(|id| !command.display_list.contains(id)),
            ListCommandKind::Add => {
                // Server insertion semantics are validated on read-back via
                // membership, since concurrent inserts may change positions.
                for id in &command.display_list {
                    if !expected.contains(id) {
                        expected.push(id.clone());
                    }
                }
            }
        }
        if expected.len() > 1000 {
            return Err(Error::InvalidInput("shared queue exceeds official window"));
        }
        state.snapshot = before;
        self.session
            .with_token(token, |owner| self.save(owner, &state, None))?;
        self.send(
            token,
            WriteIntent::Queue {
                room_id: room.into(),
                command,
                expected_display: expected,
            },
            now,
        )
        .await
    }
    pub async fn play_for(
        &self,
        token: &SessionToken,
        room: &str,
        command: PlayCommand,
        playing: bool,
        now: u64,
    ) -> Result<Option<TogetherOperation>> {
        self.report_play_for(token, room, command, playing, now, false)
            .await
    }
    pub async fn seek_for(
        &self,
        token: &SessionToken,
        room: &str,
        command: PlayCommand,
        playing: bool,
        now: u64,
    ) -> Result<Option<TogetherOperation>> {
        self.report_play_for(token, room, command, playing, now, true)
            .await
    }
    async fn report_play_for(
        &self,
        token: &SessionToken,
        room: &str,
        command: PlayCommand,
        playing: bool,
        now: u64,
        explicit_seek: bool,
    ) -> Result<Option<TogetherOperation>> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_outgoing_ready(token)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        let ids = state
            .snapshot
            .playlist
            .as_ref()
            .ok_or(Error::State("shared queue not loaded"))?
            .display
            .clone();
        let command = if explicit_seek {
            state.playback.outgoing_seek(command, &ids, playing, now)?
        } else {
            state.playback.outgoing(command, &ids, playing, now)?
        };
        if let Some(command) = command {
            // Sequence and intended command are committed in one transaction.
            self.send_with_state(
                token,
                WriteIntent::Play {
                    room_id: room.into(),
                    command,
                },
                state,
                now,
            )
            .await
            .map(Some)
        } else {
            self.session
                .with_token(token, |owner| self.save(owner, &state, None))?;
            Ok(None)
        }
    }
    pub(super) async fn send(
        &self,
        token: &SessionToken,
        intent: WriteIntent,
        now: u64,
    ) -> Result<TogetherOperation> {
        self.send_with_state(token, intent, self.state_for(token)?, now)
            .await
    }
    pub(super) async fn send_with_state(
        &self,
        token: &SessionToken,
        intent: WriteIntent,
        mut state: TogetherState,
        now: u64,
    ) -> Result<TogetherOperation> {
        let owner = self.owner(token)?;
        let mut op = TogetherOperation {
            id: uuid::Uuid::new_v4().to_string(),
            intent,
            phase: TogetherPhase::Sending,
            acknowledged: false,
            created_room: None,
            observed_room: None,
        };
        self.session
            .with_token(token, |owner| self.save(owner, &state, Some(&op)))?;
        let result = self
            .session
            .run_for(token, |api, _| async {
                let api = TogetherApi::new(api);
                match &op.intent {
                    WriteIntent::Create { refer } => api.create(refer).await.map(Some),
                    WriteIntent::Accept {
                        room_id,
                        inviter,
                        refer,
                    } => api.accept(room_id, inviter, refer).await.map(|room| {
                        Some(CreatedRoom {
                            room,
                            is_new: false,
                        })
                    }),
                    WriteIntent::End { room_id, count } => {
                        api.end(room_id, *count).await.map(|()| None)
                    }
                    WriteIntent::Queue {
                        room_id, command, ..
                    } => api.list_command(room_id, command).await.map(|()| None),
                    WriteIntent::Play { room_id, command } => {
                        api.play_command(room_id, command).await.map(|()| None)
                    }
                }
            })
            .await;
        match result {
            Ok(created) => {
                op.acknowledged = true;
                op.phase = TogetherPhase::Verifying;
                if let Some(created) = created {
                    op.observed_room = Some(created.room.room_id.clone());
                    if created.is_new {
                        op.created_room = Some(created.room);
                    } else if matches!(op.intent, WriteIntent::Create { .. }) {
                        op.phase = TogetherPhase::Rejected;
                    }
                }
            }
            Err(error) => {
                op.phase = if [200, 301, 403, 405, 488]
                    .iter()
                    .any(|code| error.is_service_code(*code))
                {
                    TogetherPhase::Rejected
                } else {
                    TogetherPhase::Uncertain
                };
                // This obligation remains with the originating account if a
                // response races logout/switch; it cannot reach the new UI.
                if op.phase == TogetherPhase::Rejected
                    && let WriteIntent::Play { command, .. } = &op.intent
                {
                    state.playback.latest = None;
                    state
                        .playback
                        .sent
                        .retain(|v| v.command.client_seq != command.client_seq);
                    self.save(&owner, &state, Some(&op))?;
                } else {
                    self.save_operation(&owner, &op)?;
                }
                return Err(error);
            }
        }
        self.save_operation(&owner, &op)?;
        if op.phase == TogetherPhase::Rejected {
            return Ok(op);
        }
        self.settle(token, op, now).await
    }
}
