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
    ) -> Result<crate::playback::controller::PlaybackFence> {
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid displayed queue revision"))?;
        let before = self
            .player
            .controller()
            .await?
            .command(token, PlaybackCommand::Snapshot)
            .await?;
        if before.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if before.fm {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        Ok(before.fence())
    }
}
