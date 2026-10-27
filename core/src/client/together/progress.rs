//! M/75 HEARTBEAT -> M/3161 reportRequest(reason="heartbeat"). This is a
//! snapshot report, not a local seek, shared-queue publication or recovery.
use super::{runtime::Context, *};
use crate::{
    listen_together::{IncomingPhase, PlayCommand, PlayState, transport::ConnectionState},
    playback::controller::PlaybackStatus,
};

impl Context {
    pub(super) async fn report_requested_progress(&self) -> Result<()> {
        if self
            .error
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .is_some()
            || self
                .connection
                .as_ref()
                .is_none_or(|v| v.borrow().state != ConnectionState::Connected)
        {
            return Ok(());
        }
        let service = TogetherService::new(self.session.clone())?;
        let state = service.state_for(&self.token)?;
        if state.room.as_ref().map(|r| r.room_id.as_str()) != Some(&self.room_id)
            || state.end_notice.is_some()
            || state
                .shared
                .as_ref()
                .is_some_and(|v| v.phase != IncomingPhase::Applied)
            || (state.role != Some(RoomRole::Waiting) && state.shared.is_none())
            || state
                .incoming
                .as_ref()
                .is_some_and(|v| matches!(v.phase, IncomingPhase::Pending | IncomingPhase::Failed))
            || service
                .operations_for(&self.token)?
                .iter()
                .any(|op| op.phase.pending())
        {
            // An inbound request never clears a failure or retries an uncertain
            // write. Explicit user recovery retains its existing entry point.
            return Ok(());
        }
        let (snapshot, queue) = self.controller.queue_with_state(&self.token).await?;
        if !matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        ) {
            return Ok(());
        }
        let Some(position) = snapshot.position else {
            return Ok(());
        };
        let shared =
            selection::shared_selection(&self.session, &self.token, self.store.clone(), &queue)?;
        let Some(target) = shared.current else {
            return Ok(());
        };
        let Some(playlist) = state.snapshot.playlist.as_ref() else {
            return Ok(());
        };
        if playlist.display != shared.display
            || playlist.mode != shared.mode
            || (shared.mode == SharedMode::Random
                && playlist.random.as_ref() != Some(&shared.random))
        {
            // Let the local semantic action publish a changed queue first.
            return Ok(());
        }
        let playing = matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Buffering
        );
        let command = PlayCommand::at_seconds(
            CommandKind::Progress,
            position,
            if playing {
                PlayState::Play
            } else {
                PlayState::Pause
            },
            &target,
            &target,
        )?;
        // Keep the official ordinary duplicate/echo filters and zero-position
        // GOTO conversion. The explicit local-seek exception must not apply.
        if let Some(result) = service
            .play_for(&self.token, &self.room_id, command, playing, now())
            .await?
            && result.phase != TogetherPhase::Confirmed
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
}
