use super::*;
use crate::{
    catalog::CatalogApi,
    listen_together::{IncomingPhase, SharedReception, Version},
    playback::controller::PlaybackSnapshot,
};
mod plan;

impl incoming::Receiver<'_> {
    pub async fn refresh_shared(
        &self,
        version: Option<&[Version]>,
        ignore_play: bool,
    ) -> Result<Option<PlaybackSnapshot>> {
        let (before, queue) = self.controller.queue_with_state(self.token).await?;
        let service = TogetherService::new(self.session.clone())?;
        let receipt = service
            .receive_shared_for(self.token, self.room, version, ignore_play, now())
            .await?;
        let expected_revision = before.control_revision;
        let result = self.install_shared(&receipt, before, queue).await;
        let phase = match &result {
            Ok(after) if after.control_revision == expected_revision.saturating_add(1) => {
                IncomingPhase::Applied
            }
            Ok(_) => IncomingPhase::Superseded,
            Err(error)
                if matches!(error.root(), Error::QueueChanged)
                    || (matches!(error.root(), Error::Cancelled)
                        && self.controller.snapshot().control_revision
                            > expected_revision.saturating_add(1)) =>
            {
                IncomingPhase::Superseded
            }
            Err(_) => IncomingPhase::Failed,
        };
        service.finish_shared_for(self.token, self.room, &receipt.id, phase)?;
        if phase == IncomingPhase::Superseded {
            Ok(None)
        } else {
            result.map(Some)
        }
    }
    async fn install_shared(
        &self,
        receipt: &SharedReception,
        before: PlaybackSnapshot,
        queue: crate::playback::QueueSnapshot,
    ) -> Result<PlaybackSnapshot> {
        let playlist = receipt
            .snapshot
            .as_ref()
            .and_then(|s| s.playlist.as_ref())
            .ok_or(Error::State("shared reception has no queue"))?;
        let songs = self
            .session
            .run_for(self.token, |api, _| async {
                CatalogApi::new(api)
                    .songs(
                        &playlist
                            .display
                            .iter()
                            .map(|id| json!(id))
                            .collect::<Vec<_>>(),
                        500,
                    )
                    .await
            })
            .await?;
        let shared =
            selection::shared_selection(self.session, self.token, self.store.clone(), &queue)?;
        let cards = playlist
            .display
            .iter()
            .zip(songs.songs)
            .filter_map(|(id, song)| song.map(|s| (id.clone(), s)))
            .collect::<std::collections::BTreeMap<_, _>>();
        let update = plan::build(receipt, &before, &queue, &shared, &cards)?;
        self.session.with_token(self.token, |uid| {
            self.store.put(
                uid,
                "listen-together-cards-v1",
                "current",
                &serde_json::to_value(&cards)?,
            )
        })?;
        self.controller
            .command(self.token, PlaybackCommand::UpdateQueue(update))
            .await
    }
}
