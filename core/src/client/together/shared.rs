use super::*;
use crate::{
    catalog::CatalogApi,
    listen_together::{IncomingPhase, SharedReception, Version},
    playback::{
        QueueSnapshot,
        controller::{PlaybackSnapshot, PlaybackStatus, QueueUpdate},
    },
};
mod plan;

impl incoming::Receiver<'_> {
    pub async fn refresh_shared(
        &self,
        version: Option<&[Version]>,
        ignore_play: bool,
    ) -> Result<Option<PlaybackSnapshot>> {
        let owner = self
            .token
            .uid
            .as_deref()
            .ok_or(Error::State("no account"))?;
        let (before, queue) = self.controller.queue_with_state(self.token).await?;
        let service = TogetherService::new(self.session.clone())?;
        let previous = service.state_for(self.token)?;
        let prior = previous.shared.filter(|_| {
            !previous.incoming.as_ref().is_some_and(|pending| {
                matches!(
                    pending.phase,
                    IncomingPhase::Pending | IncomingPhase::Failed
                )
            })
        });
        let receipt = service
            .receive_shared_for(self.token, self.room, version, ignore_play, now())
            .await?;
        let expected_revision = before.control_revision;
        let result = async {
            let update = self.plan_shared(&receipt, &before, &queue).await?;
            let identical = update.queue == queue
                && prior
                    .as_ref()
                    .is_some_and(|old| old.already_applied_as(&receipt, owner));
            let applied = self
                .controller
                .command(self.token, PlaybackCommand::UpdateQueue(update))
                .await;
            if identical
                && matches!(&applied, Err(error) if matches!(error.root(), Error::QueueChanged))
            {
                let (after, current_queue) = self.controller.queue_with_state(self.token).await?;
                if current_queue == queue && same_source_new_transport(&before, &after) {
                    // The room already authorized this exact queue and source.
                    // A duplicate refresh raced a local pause/seek. Keep the
                    // newer control and let the runtime report it normally.
                    return Ok((after, true));
                }
            }
            applied.map(|after| (after, false))
        }
        .await;
        let phase = match &result {
            Ok((after, reused))
                if *reused || after.control_revision == expected_revision.saturating_add(1) =>
            {
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
            result.map(|(snapshot, _)| Some(snapshot))
        }
    }
    async fn plan_shared(
        &self,
        receipt: &SharedReception,
        before: &PlaybackSnapshot,
        queue: &QueueSnapshot,
    ) -> Result<QueueUpdate> {
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
            selection::shared_selection(self.session, self.token, self.store.clone(), queue)?;
        let cards = playlist
            .display
            .iter()
            .zip(songs.songs)
            .filter_map(|(id, song)| song.map(|s| (id.clone(), s)))
            .collect::<std::collections::BTreeMap<_, _>>();
        let update = plan::build(receipt, before, queue, &shared, &cards)?;
        self.session.with_token(self.token, |uid| {
            self.store.put(
                uid,
                "listen-together-cards-v1",
                "current",
                &serde_json::to_value(&cards)?,
            )
        })?;
        Ok(update)
    }
}

fn same_source_new_transport(before: &PlaybackSnapshot, after: &PlaybackSnapshot) -> bool {
    before.account == after.account
        && before.play_id.is_some()
        && before.play_id == after.play_id
        && before.selected.is_some()
        && before.selected == after.selected
        && before.queue_revision == after.queue_revision
        && before.mode == after.mode
        && before
            .timeline
            .as_ref()
            .zip(after.timeline.as_ref())
            .is_some_and(|(before, after)| {
                (
                    before.mode,
                    before.start,
                    before.end,
                    before.length,
                    before.total,
                    before.media_start,
                ) == (
                    after.mode,
                    after.start,
                    after.end,
                    after.length,
                    after.total,
                    after.media_start,
                )
            })
        && after.control_revision > before.control_revision
        && matches!(
            before.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused
        )
        && matches!(
            after.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused
        )
        && (before.paused != after.paused
            || (!after.native_seeking && after.seek_revision > before.seek_revision))
}
