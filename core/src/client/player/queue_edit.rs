//! Edits from a displayed queue keep its revision through native actor commit.
use super::*;

impl QueueEdit {
    pub(super) fn into_action(self) -> PlayerAction {
        match self {
            Self::Select { entry_id } => PlayerAction::Select { entry_id },
            Self::Remove { entry_ids } => PlayerAction::Remove { entry_ids },
            Self::Reorder { entry_ids } => PlayerAction::Reorder { entry_ids },
            Self::Clear => PlayerAction::Clear,
        }
    }
}

impl MusicClient {
    pub(super) async fn displayed_queue_fence(
        &self,
        token: &SessionToken,
        revision: &str,
        action: &mut PlayerAction,
    ) -> Result<crate::playback::controller::PlaybackFence> {
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
        Ok(before.fence())
    }
}
