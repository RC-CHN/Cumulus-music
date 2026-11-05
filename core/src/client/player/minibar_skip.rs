//! Only the official main bottom bar's explicit forward/back triggers.
use super::{MusicClient, PlayerView, project};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        Step,
        controller::{PlaybackCommand, PlaybackController, PlaybackSnapshot},
        engine::Cancellation,
    },
};

use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn step_ended_minibar(
        &self,
        epoch: &str,
        receipt: &str,
        entry_id: &str,
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
        if current
            .ended_minibar_receipt
            .is_none_or(|id| id.simple().to_string() != receipt)
            || current
                .selected
                .as_ref()
                .is_none_or(|row| row.entry_id != entry_id)
            || current.queue_revision.to_string() != revision
        {
            return Err(Error::StaleResponse);
        }
        let focus = self.focus.clone();
        let owner = focus.snapshot()?;
        let room = self.together_detached_guard(&token);
        if owner.owner != Some(super::super::focus::Owner::Audio) || room.is_cancelled() {
            return Err(Error::StaleResponse);
        }

        // A validated explicit click wins against older transport work
        // without reclaiming focus or invalidating a newer pending selection.
        let attempt = observed
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        selection
            .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        let transport = Cancellation::new(move || {
            selection.load(Ordering::Acquire) == attempt
                && focus.check_snapshot(owner).is_ok()
                && !room.is_cancelled()
        });
        let Some(verified) = self
            .player
            .ordinary_skip
            .admit(&token, transport.clone())?
            .filter(|value| value.groups.group("PH-PC-skipSongs2").is_some())
        else {
            // No fresh signed/connectivity authority for this specialized
            // receipt. Never substitute an unscoped Next on an ended row.
            return Err(Error::StaleResponse);
        };
        let fence = current.fence();
        let authority = verified.authority.clone();
        let outcome = controller
            .verified_minibar_skip(&token, fence.clone(), direction, verified, true)
            .await?;
        transport.check()?;
        let state = if let Some(state) = outcome.applied {
            state
        } else {
            controller
                .command_guarded(
                    &token,
                    match direction {
                        Step::Next => PlaybackCommand::Next,
                        Step::Previous => PlaybackCommand::Previous,
                    },
                    fence,
                    authority,
                )
                .await?
        };
        self.session.check_generation(&token)?;
        project(&state, &token)
    }

    pub(super) async fn try_minibar_skip(
        &self,
        token: &SessionToken,
        controller: &PlaybackController,
        current: &PlaybackSnapshot,
        direction: Step,
        transport: &Cancellation,
    ) -> Result<Option<PlaybackSnapshot>> {
        transport.check()?;
        if current.fm || current.intelligence.is_some() {
            return Ok(None);
        }
        let Some(verified) = self.player.ordinary_skip.admit(token, transport.clone())? else {
            return Ok(None);
        };
        let outcome = controller
            .verified_minibar_skip(token, current.fence(), direction, verified, false)
            .await?;
        transport.check()?;
        // An in-flight preflight failure isn't permission to send a second
        // transport intent. Only a decision of Unchanged falls through.
        Ok(outcome.applied)
    }
}
