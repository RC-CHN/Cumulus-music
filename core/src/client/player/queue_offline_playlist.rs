//! Detail menu Add All for a displayed owned cache. No online source resolution.
use super::{super::focus::Owner, *};
use crate::playback::controller::OfflinePlaylistQueueUpdate;
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn queue_offline_playlist(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
        cached_at_ms: u64,
        expected_revision: &str,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision.to_string() != expected_revision {
            return Err(Error::QueueChanged);
        }
        if before.fm || queue.intelligence.is_some() || self.together_attached(&token, None)? {
            return Err(Error::State(
                "cached playlist queue requires ordinary playback",
            ));
        }
        let focus = self.focus.snapshot()?;
        let observed = self.player.selection.load(Ordering::Acquire);
        let prepared = self
            .offline_playlist_queue(&token, id, revision, cached_at_ms, None, false)
            .await?;
        let start = queue.items.is_empty();
        let current = queue.current.clone();
        queue = super::queue_playlist::insert_collection_by_key(
            queue,
            prepared.resources,
            prepared.source,
            true,
            false,
            |row| {
                let downloaded = super::identity::kind(row) == PlayerResourceKind::Downloaded;
                super::identity::collection_key(row, downloaded)
            },
        );
        if !start {
            // A nonempty queue with no selection still does not request a start.
            queue.current = current;
        }
        if prepared.guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)? {
            return Err(Error::State(
                "cached playlist queue requires ordinary playback",
            ));
        }

        let expected = before.fence();
        let (lease, attempt) = if start {
            let current = controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            if current.fence() != expected || prepared.guard.is_cancelled() {
                return Err(Error::QueueChanged);
            }
            let (lease, attempt) = self.reserve_prepared_audio(&token, focus, || {
                if controller.snapshot().fence() != expected {
                    return Err(Error::QueueChanged);
                }
                let next = observed
                    .checked_add(1)
                    .ok_or(Error::State("playback selection exhausted"))?;
                self.player
                    .selection
                    .compare_exchange(observed, next, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| Error::StaleResponse)?;
                Ok(next)
            })?;
            self.finish_media_claim(&token, Owner::Audio, lease).await?;
            (Some(lease), attempt)
        } else {
            (None, observed)
        };
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        let result = controller
            .command_guarded(
                &token,
                PlaybackCommand::ApplyOfflinePlaylistQueue(OfflinePlaylistQueueUpdate {
                    expected: expected.clone(),
                    queue,
                    start,
                    binding: prepared.binding,
                    index: prepared._local_lease,
                }),
                expected,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && lease.is_none_or(|lease| focus.check(lease, Owner::Audio).is_ok())
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&result, &token)
    }
}
