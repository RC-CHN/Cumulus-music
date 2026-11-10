//! Atomic detail-menu insertion for a cached or duration-sorted playlist projection.
use super::{super::focus::Owner, playlist_duration::BoundPlaylist, *};
use crate::playback::controller::OfflinePlaylistQueueUpdate;
use crate::{client::dto::PlaylistDurationScope, membership::Quality};
use std::sync::atomic::Ordering;

enum Source<'a> {
    Offline {
        id: &'a str,
        revision: &'a str,
        cached_at_ms: u64,
    },
    Sorted(PlaylistDurationScope, Quality),
}

impl MusicClient {
    pub(super) async fn queue_sorted_playlist(
        &self,
        epoch: &str,
        scope: PlaylistDurationScope,
        quality: Quality,
        expected_revision: &str,
    ) -> Result<PlayerView> {
        self.queue_bound_playlist(epoch, Source::Sorted(scope, quality), expected_revision)
            .await
    }

    pub(super) async fn queue_offline_playlist(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
        cached_at_ms: u64,
        expected_revision: &str,
    ) -> Result<PlayerView> {
        self.queue_bound_playlist(
            epoch,
            Source::Offline {
                id,
                revision,
                cached_at_ms,
            },
            expected_revision,
        )
        .await
    }

    async fn queue_bound_playlist(
        &self,
        epoch: &str,
        source: Source<'_>,
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
                "bound playlist queue requires ordinary playback",
            ));
        }
        let focus = self.focus.snapshot()?;
        let observed = self.player.selection.load(Ordering::Acquire);
        let prepared: BoundPlaylist = match source {
            Source::Offline {
                id,
                revision,
                cached_at_ms,
            } => self
                .offline_playlist_queue(&token, id, revision, cached_at_ms, None, false)
                .await?
                .into(),
            Source::Sorted(scope, quality) => {
                self.sorted_playlist_resources(&token, &scope, None, false, quality)
                    .await?
            }
        };
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
                "bound playlist queue requires ordinary playback",
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
                    index: prepared.index_lease,
                }),
                expected,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && !prepared.receipt_guard.is_cancelled()
                        && lease.is_none_or(|lease| focus.check(lease, Owner::Audio).is_ok())
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&result, &token)
    }
}
