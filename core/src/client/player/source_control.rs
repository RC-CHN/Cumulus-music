use super::{MusicClient, PlayerView, project};
use crate::{
    Error, Result,
    playback::{
        Step,
        controller::{PlaybackCommand, PlaybackSnapshot, PlaybackStatus},
        engine::Cancellation,
    },
};
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn step_loading_source(
        &self,
        epoch: &str,
        play_id: &str,
        revision: &str,
        direction: Step,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let selection = self.player.selection.clone();
        let observed = selection.load(Ordering::Acquire);
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        validate_identity(&current, play_id, revision)?;
        if current.visible_queue_len < 2
            || current.fm
            || current.intelligence.is_some()
            || !matches!(
                current.state,
                PlaybackStatus::Preparing
                    | PlaybackStatus::Loading
                    | PlaybackStatus::Playing
                    | PlaybackStatus::Buffering
                    | PlaybackStatus::Paused
            )
        {
            return Err(Error::State("loading step requires an ordinary queue"));
        }
        let focus = self.focus.clone();
        let owner = focus.snapshot()?;
        let room = self.together_detached_guard(&token);
        if owner.owner != Some(super::super::focus::Owner::Audio) || room.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        // Reserve an explicit replacement only after validating the old
        // selection. The actor fences again before it cancels that decoder.
        let attempt = observed
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        selection
            .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        let state = controller
            .command_guarded(
                &token,
                match direction {
                    Step::Next => PlaybackCommand::Next,
                    Step::Previous => PlaybackCommand::Previous,
                },
                current.fence(),
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check_snapshot(owner).is_ok()
                        && !room.is_cancelled()
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&state, &token)
    }
}

pub(super) fn validate_identity(
    current: &PlaybackSnapshot,
    play_id: &str,
    queue_revision: &str,
) -> Result<()> {
    let revision = queue_revision
        .parse::<u64>()
        .map_err(|_| Error::InvalidInput("invalid source playback queue revision"))?;
    if revision.to_string() != queue_revision {
        return Err(Error::InvalidInput(
            "invalid source playback queue revision",
        ));
    }
    if current.queue_revision != revision || current.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

pub(super) fn validate(
    current: &PlaybackSnapshot,
    play_id: &str,
    queue_revision: &str,
    refresh_failed: bool,
) -> Result<()> {
    validate_identity(current, play_id, queue_revision)?;
    if refresh_failed {
        return if current.state == PlaybackStatus::Error {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        };
    }
    if !matches!(
        current.state,
        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
    ) {
        return Err(Error::State("source playback is no longer loaded"));
    }
    Ok(())
}
