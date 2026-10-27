use super::*;
use crate::listen_together::{IncomingPhase, IncomingPlayback, PlayCommand, PlayState};
use crate::playback::controller::{PlaybackSnapshot, PlaybackStatus, RemotePlayback};

pub(super) struct Receiver<'a> {
    pub session: &'a Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub controller: &'a PlaybackController,
    pub token: &'a SessionToken,
    pub room: &'a str,
}
impl Receiver<'_> {
    fn service(&self) -> Result<TogetherService> {
        TogetherService::new(self.session.clone())
    }
    pub async fn receive(&self, command: PlayCommand) -> Result<Option<PlaybackSnapshot>> {
        let (snapshot, queue) = self.controller.queue_with_state(self.token).await?;
        let shared =
            selection::shared_selection(self.session, self.token, self.store.clone(), &queue)?;
        let service = self.service()?;
        let Some(incoming) = service.receive_play_for(
            self.token,
            self.room,
            command,
            shared.current.as_deref(),
            playing(&snapshot),
            now(),
        )?
        else {
            return Ok(None);
        };
        let target = target_entry(&queue, &shared, &incoming.command.target_song_id);
        self.apply(incoming, snapshot, target).await
    }
    /// Explicit recovery checks the authoritative snapshot. A saved older
    /// command must not be replayed after the room has advanced elsewhere.
    pub async fn retry(&self) -> Result<Option<PlaybackSnapshot>> {
        // Reserve the native intent before network reads. A local operation
        // during refresh must invalidate the fence instead of being overwritten.
        let (snapshot, queue) = self.controller.queue_with_state(self.token).await?;
        let service = self.service()?;
        let state = service.refresh_for(self.token, now()).await?;
        if state.room.as_ref().map(|r| r.room_id.as_str()) != Some(self.room) {
            return Err(Error::State("together room changed"));
        }
        let Some(incoming) = state
            .incoming
            .filter(|v| matches!(v.phase, IncomingPhase::Pending | IncomingPhase::Failed))
        else {
            return Ok(None);
        };
        let sequence = incoming
            .command
            .server_seq
            .ok_or(Error::Response("incoming command lacks server sequence"))?;
        if state.snapshot.play_command.as_ref() != Some(&incoming.command)
            || state.role == Some(RoomRole::Waiting)
            || state
                .snapshot
                .playlist
                .as_ref()
                .is_none_or(|p| !p.display.contains(&incoming.command.target_song_id))
        {
            service.finish_incoming_for(
                self.token,
                self.room,
                sequence,
                IncomingPhase::Superseded,
            )?;
            return Ok(None);
        }
        let shared =
            selection::shared_selection(self.session, self.token, self.store.clone(), &queue)?;
        let target = target_entry(&queue, &shared, &incoming.command.target_song_id);
        self.apply(incoming, snapshot, target).await
    }
    async fn apply(
        &self,
        incoming: IncomingPlayback,
        snapshot: PlaybackSnapshot,
        target: Option<String>,
    ) -> Result<Option<PlaybackSnapshot>> {
        let service = self.service()?;
        let sequence = incoming
            .command
            .server_seq
            .ok_or(Error::Response("incoming command lacks server sequence"))?;
        let Some(entry_id) = target else {
            service.finish_incoming_for(self.token, self.room, sequence, IncomingPhase::Failed)?;
            return Err(Error::State("remote track is not in the native queue"));
        };
        let selecting = snapshot
            .selected
            .as_ref()
            .is_none_or(|v| v.entry_id != entry_id)
            || !matches!(
                snapshot.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            );
        let paused = match incoming.command.command_type {
            CommandKind::Play => false,
            CommandKind::Pause => true,
            _ if selecting => incoming.command.play_status == PlayState::Pause,
            _ => !playing(&snapshot),
        };
        let expected = snapshot.fence();
        let result = self
            .controller
            .command(
                self.token,
                PlaybackCommand::ApplyRemote(RemotePlayback {
                    expected: expected.clone(),
                    entry_id,
                    position: incoming.command.progress as f64 / 1000.0,
                    paused,
                }),
            )
            .await;
        match result {
            Ok(value) => {
                // A local pause accepted while decoding wins. It is a later
                // control revision even if the load itself eventually succeeds.
                let phase = if value.control_revision == expected.control_revision.saturating_add(1)
                {
                    IncomingPhase::Applied
                } else {
                    IncomingPhase::Superseded
                };
                service.finish_incoming_for(self.token, self.room, sequence, phase)?;
                if phase == IncomingPhase::Applied {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
            Err(error) => {
                let newer = self.controller.snapshot().control_revision
                    > expected.control_revision.saturating_add(1);
                let superseded = matches!(error.root(), Error::QueueChanged)
                    || (newer && matches!(error.root(), Error::Cancelled));
                service.finish_incoming_for(
                    self.token,
                    self.room,
                    sequence,
                    if superseded {
                        IncomingPhase::Superseded
                    } else {
                        IncomingPhase::Failed
                    },
                )?;
                if superseded { Ok(None) } else { Err(error) }
            }
        }
    }
}
fn target_entry(
    queue: &crate::playback::QueueSnapshot,
    shared: &selection::Shareable,
    song: &str,
) -> Option<String> {
    // The native queue may contain two instances of the same song. A same-song
    // progress update must retain its selected instance and avoid a reload.
    queue
        .current
        .iter()
        .chain(queue.items.iter().map(|entry| &entry.entry_id))
        .find(|id| shared.entries.get(*id).is_some_and(|value| value == song))
        .cloned()
}
fn playing(snapshot: &PlaybackSnapshot) -> bool {
    matches!(
        snapshot.state,
        PlaybackStatus::Playing | PlaybackStatus::Buffering
    )
}
