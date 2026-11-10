//! Play or enqueue the same complete native projection the playlist table displays.
use super::{
    super::{focus::Owner, playlist_offline::OfflineQueue},
    queue_click::{RetainedRow, TableSong},
    *,
};
use crate::{
    client::dto::PlaylistDurationScope,
    library::LocalResource,
    membership::Quality,
    playlist::{PlaylistLocalRef, PlaylistLocalStore, offline_snapshot::PlaylistQueueBinding},
    storage::OperationLock,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::atomic::Ordering};

pub(super) struct BoundPlaylist {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub binding: PlaylistQueueBinding,
    pub guard: Cancellation,
    pub receipt_guard: Cancellation,
    pub index_lease: Option<OperationLock>,
}

impl From<OfflineQueue> for BoundPlaylist {
    fn from(value: OfflineQueue) -> Self {
        Self {
            resources: value.resources,
            index: value.index,
            source: value.source,
            binding: PlaylistQueueBinding::Offline(value.binding),
            guard: value.guard,
            receipt_guard: Cancellation::default(),
            index_lease: value._local_lease,
        }
    }
}

fn resource_identity(value: &Value) -> Option<(bool, &str)> {
    if let Some(id) = value["local_id"].as_str() {
        Some((true, id))
    } else {
        value["id"].as_str().map(|id| (false, id))
    }
}

impl MusicClient {
    pub(super) async fn sorted_playlist_resources(
        &self,
        token: &SessionToken,
        scope: &PlaylistDurationScope,
        start: Option<&MixedPlaylistStart>,
        selected_only: bool,
        quality: Quality,
    ) -> Result<BoundPlaylist> {
        let prepared = self.playlist_duration.get(token, &scope.receipt)?;
        let host = self.playlist_duration.clone();
        let captured = prepared.clone();
        let receipt_guard = Cancellation::new(move || host.current(&captured));
        let page = &prepared.projection.page;
        let playlist = page.playlist.as_ref().ok_or(Error::QueueChanged)?;
        let matches_start =
            |local, id: &str| start.is_none_or(|row| row.local == local && row.id == id);
        if let Some(cached_at_ms) = prepared.source.cached_at_ms {
            let revision = prepared
                .source
                .local_revision
                .as_deref()
                .ok_or(Error::QueueChanged)?;
            let mut bound: BoundPlaylist = self
                .offline_playlist_queue(
                    token,
                    &playlist.id,
                    revision,
                    cached_at_ms,
                    start.map(|row| (row.id.as_str(), row.local)),
                    selected_only,
                )
                .await?
                .into();
            let mut rank = HashMap::new();
            for (position, &ordinal) in prepared.ordinals(scope.order).iter().enumerate() {
                let track = &page.tracks[ordinal];
                rank.entry((track.local, track.id.as_str()))
                    .or_insert(position);
            }
            bound.resources.sort_by_key(|value| {
                resource_identity(value)
                    .and_then(|id| rank.get(&id))
                    .copied()
                    .unwrap_or(usize::MAX)
            });
            if bound.resources.iter().any(|resource| {
                resource_identity(resource).is_none_or(|id| !rank.contains_key(&id))
            }) {
                return Err(Error::QueueChanged);
            }
            bound.index = bound
                .resources
                .iter()
                .position(|resource| {
                    resource_identity(resource).is_some_and(|(local, id)| matches_start(local, id))
                })
                .ok_or(Error::QueueChanged)?;
            let combined = receipt_guard.clone();
            let cache_guard = bound.guard;
            bound.guard =
                Cancellation::new(move || !combined.is_cancelled() && !cache_guard.is_cancelled());
            bound.receipt_guard = receipt_guard;
            return Ok(bound);
        }
        let membership = prepared
            .source
            .local_revision
            .as_ref()
            .map(|_| prepared.generation);
        let store = PlaylistLocalStore::new(self.session.clone())?;
        let id = playlist.id.clone();
        let token_owned = token.clone();
        let check_id = id.clone();
        let check_receipt = receipt_guard.clone();
        let session = self.session.clone();
        let guard = Cancellation::new(move || {
            !check_receipt.is_cancelled()
                && session.check_generation(&token_owned).is_ok()
                && membership.is_none_or(|generation| {
                    store
                        .generation(&token_owned, &check_id)
                        .is_ok_and(|v| v == generation)
                })
        });
        if guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        // Normal playlist trials are inferred from the verified ID; favorites
        // and the separate vertical-zone path are not sorted by this entry.
        let trial = self.ordinary_playlist_trial(token, &id, &json!({"id": id}))?;
        let mut source =
            super::origin::named(json!({"kind":"playlist","id":id}), Some(&playlist.name));
        source["history_resource"] = json!({"id":id,"name":playlist.name,"trackCount":page.total});
        if let Some(trial) = trial {
            source["trial_mode"] = json!(trial);
        }
        let refs = page
            .tracks
            .iter()
            .filter(|track| {
                track.local && !track.missing && (!selected_only || matches_start(true, &track.id))
            })
            .map(|track| {
                let revision = page
                    .playlist_local
                    .as_ref()
                    .and_then(|local| local.rows.get(&track.id))
                    .ok_or(Error::QueueChanged)?;
                Ok(PlaylistLocalRef {
                    id: track.id.clone(),
                    revision: revision.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let (mut indexed, index_lease) = if refs.is_empty() {
            (Default::default(), None)
        } else {
            let runtime = self.local.context().await?;
            let (items, lease) =
                tokio::task::spawn_blocking(move || runtime.library.hold_playlist_items(&refs))
                    .await
                    .map_err(|_| Error::State("sorted playlist index worker stopped"))??;
            (items, Some(lease))
        };
        let mut resources = Vec::new();
        let mut index = None;
        for &ordinal in prepared.ordinals(scope.order) {
            let track = &page.tracks[ordinal];
            if track.missing || track.voice_id.is_some() {
                continue;
            }
            let selected = matches_start(track.local, &track.id);
            if selected_only && !selected {
                continue;
            }
            if selected && index.is_none() {
                index = Some(resources.len());
            }
            let row = if track.local {
                let item = indexed.remove(&track.id).ok_or(Error::QueueChanged)?;
                serde_json::to_value(LocalResource {
                    local_id: item.id,
                    local_revision: item.revision,
                    metadata: item.metadata,
                })?
            } else {
                let mut row = super::queue_playlist::queue_resource_for_track(track, quality);
                if let Some(trial) = trial {
                    row["trial_mode"] = json!(trial);
                }
                row
            };
            resources.push(row);
        }
        let index = index.ok_or(Error::State("sorted playlist has no available songs"))?;
        Ok(BoundPlaylist {
            resources,
            index,
            source,
            guard,
            receipt_guard,
            binding: PlaylistQueueBinding::Online { id, membership },
            index_lease,
        })
    }

    pub(super) async fn play_sorted_playlist(
        &self,
        epoch: &str,
        scope: PlaylistDurationScope,
        start: Option<MixedPlaylistStart>,
        double_click: bool,
        quality: Quality,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if before.fm || queue.intelligence.is_some() || self.together_attached(&token, None)? {
            return Err(Error::State("sorted playlist requires ordinary playback"));
        }
        let expected = before.fence();
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let retain =
            double_click && start.is_some() && !self.replace_queue_on_double_click(&token)?;
        let prepared = self
            .sorted_playlist_resources(&token, &scope, start.as_ref(), retain, quality)
            .await?;
        if prepared.guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let retained = if retain {
            if self.player.selection.load(Ordering::Acquire) != observed {
                return Err(Error::StaleResponse);
            }
            let snapshot = self.queue_click_snapshot(&token).await?;
            if !snapshot.matches(&expected, observed) {
                return Err(Error::QueueChanged);
            }
            Some(snapshot)
        } else {
            None
        };
        let (lease, attempt) = if let Some(snapshot) = retained.as_ref() {
            (
                self.claim_native_media(&token, Owner::Audio, snapshot.focus)
                    .await?,
                snapshot.attempt,
            )
        } else {
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
            (lease, attempt)
        };
        let resource = &prepared.resources[prepared.index];
        let (policy, song) = if resource["local_id"].is_string() {
            (RetainedRow::Local, TableSong::IndexedLocal(resource))
        } else if resource
            .get(crate::download::offline::OFFLINE_PLAYLIST_DOWNLOAD)
            .is_some()
        {
            (
                RetainedRow::OfflineDownload,
                TableSong::OfflineDownload(resource),
            )
        } else {
            (
                RetainedRow::Catalog,
                TableSong::Catalog(resource["id"].as_str().ok_or(Error::QueueChanged)?),
            )
        };
        if double_click
            && let Some(state) = self
                .toggle_active_song_row(
                    &token,
                    song,
                    attempt,
                    lease,
                    &prepared.guard,
                    retained.as_ref(),
                )
                .await?
        {
            self.session.check_generation(&token)?;
            return project(&state, &token);
        }
        let state = if let Some(snapshot) = retained {
            self.play_retained_queue_row_with_source(
                &token,
                snapshot,
                (resource.clone(), prepared.source),
                lease,
                policy,
                Some(prepared.guard),
            )
            .await?
        } else {
            let _room = self
                .together
                .gate
                .try_lock()
                .map_err(|_| Error::MutationBusy)?;
            if self.together_attached(&token, None)? {
                return Err(Error::QueueChanged);
            }
            self.submit_retained_row(
                &token,
                PlaybackCommand::Replace {
                    resources: prepared.resources,
                    index: prepared.index,
                    source: prepared.source,
                },
                expected,
                attempt,
                lease,
                Some(prepared.guard),
            )
            .await?
        };
        self.session.check_generation(&token)?;
        project(&state, &token)
    }
}
