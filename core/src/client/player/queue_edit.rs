//! Edits from a displayed queue keep its revision through native actor commit.
use super::*;

impl QueueEdit {
    pub(super) fn into_action(self) -> (PlayerAction, Option<crate::playback::QueueMovement>) {
        match self {
            Self::Select { entry_id } => (PlayerAction::Select { entry_id }, None),
            Self::Remove { entry_ids } => (PlayerAction::Remove { entry_ids }, None),
            Self::Reorder { entry_ids } => (PlayerAction::Reorder { entry_ids }, None),
            Self::Move {
                entry_ids,
                movement,
            } => (PlayerAction::Reorder { entry_ids }, Some(movement)),
            Self::Clear => (PlayerAction::Clear, None),
        }
    }
}

impl MusicClient {
    pub(super) fn removal_command(
        &self,
        token: &SessionToken,
        current: &PlaybackSnapshot,
        displayed: bool,
        entry_ids: Vec<String>,
    ) -> Result<PlaybackCommand> {
        if !displayed
            || current.state != crate::playback::controller::PlaybackStatus::Paused
            || current.intelligence.is_some()
            || !current
                .selected
                .as_ref()
                .is_some_and(|selected| entry_ids.contains(&selected.entry_id))
        {
            return Ok(PlaybackCommand::Remove(entry_ids));
        }
        // The displayed edit holds the room gate. An attached or persisted
        // room, or media ownership belonging to another surface, must never
        // acquire an independent ordinary-playback resume.
        let audio = self.focus.snapshot()?;
        if audio.owner != Some(super::super::focus::Owner::Audio)
            || self.queue_clear_context(token)?.room_id.is_some()
        {
            return Ok(PlaybackCommand::Remove(entry_ids));
        }
        let focus = self.focus.clone();
        Ok(PlaybackCommand::RemoveAndResume {
            entry_ids,
            audio_owner: Cancellation::new(move || focus.check_snapshot(audio).is_ok()),
        })
    }

    pub(super) async fn displayed_queue_fence(
        &self,
        token: &SessionToken,
        revision: &str,
        action: &mut PlayerAction,
        movement: Option<&crate::playback::QueueMovement>,
    ) -> Result<(crate::playback::controller::PlaybackFence, bool)> {
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid displayed queue revision"))?;
        let (before, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(token)
            .await?;
        if before.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if before.fm {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        if let (PlayerAction::Reorder { entry_ids }, Some(movement)) = (&*action, movement) {
            let order = movement.visible_order(&queue)?;
            if !order
                .iter()
                .copied()
                .eq(entry_ids.iter().map(String::as_str))
            {
                return Err(Error::InvalidInput(
                    "displayed order does not match the queue movement",
                ));
            }
            let no_change = !movement.can_demote(&queue)
                && order.into_iter().eq(queue
                    .items
                    .iter()
                    .filter(|item| item.visible_in_queue())
                    .map(|item| item.entry_id.as_str()));
            // The actor derives its order again inside this exact fence.
            return Ok((before.fence(), no_change));
        }
        if queue.items.iter().any(|item| !item.visible_in_queue()) {
            let visible = queue
                .items
                .iter()
                .filter(|item| item.visible_in_queue())
                .map(|item| item.entry_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            match action {
                PlayerAction::Select { entry_id } if !visible.contains(entry_id.as_str()) => {
                    return Err(Error::InvalidInput("entry is outside the displayed queue"));
                }
                PlayerAction::Remove { entry_ids } => {
                    if entry_ids.iter().any(|id| !visible.contains(id.as_str())) {
                        return Err(Error::InvalidInput("entry is outside the displayed queue"));
                    }
                }
                PlayerAction::Reorder { entry_ids } => {
                    if entry_ids.len() != visible.len()
                        || entry_ids
                            .iter()
                            .map(String::as_str)
                            .collect::<std::collections::HashSet<_>>()
                            != visible
                    {
                        return Err(Error::InvalidInput(
                            "reordering must retain every visible entry exactly once",
                        ));
                    }
                    // Preserve native-only slots while reordering the displayed rows.
                    // Neither the hidden identity nor its resource is supplied by UI.
                    let mut ordered = entry_ids.iter();
                    *entry_ids = queue
                        .items
                        .iter()
                        .map(|item| {
                            if item.visible_in_queue() {
                                ordered.next().unwrap().clone()
                            } else {
                                item.entry_id.clone()
                            }
                        })
                        .collect();
                }
                _ => {}
            }
        }
        Ok((before.fence(), false))
    }
}
