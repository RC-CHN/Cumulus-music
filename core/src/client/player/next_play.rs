//! Song menu nextPlay: insert after the current entry without replacing playback.
//! Official playingList.addItemToCurPlayingList deduplicates by resource identity.
use super::*;
use crate::{
    catalog::CatalogApi,
    membership::Quality,
    playback::{
        QueueEntry,
        controller::{QueueTransport, QueueUpdate},
    },
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub(super) enum NextPlayOrigin {
    Song,
    Search,
}

pub(super) fn search_track_source() -> Value {
    json!({"kind":"songs","scene":"search","trial_mode":42})
}

impl MusicClient {
    pub(super) async fn next_play(
        &self,
        epoch: &str,
        id: String,
        quality: Quality,
        expected_revision: String,
        origin: NextPlayOrigin,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let id = identifier(&json!(id))?;
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(&token).await?;
        if expected_revision != before.queue_revision.to_string() {
            return Err(Error::QueueChanged);
        }
        if before.fm {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        // Selecting the current song does not duplicate or restart it.
        if before
            .selected
            .as_ref()
            .is_some_and(|e| super::identity::online_song(&e.resource, &id))
        {
            return project(&before, &token);
        }
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let batch = self
            .session
            .run_for(&token, |api, _| {
                let id = id.clone();
                async move { CatalogApi::new(api).songs(&[json!(id)], 100).await }
            })
            .await?;
        let song = batch
            .songs
            .into_iter()
            .next()
            .flatten()
            .ok_or(Error::State("selected song is unavailable"))?;
        let view = mapping::track(&song)?;
        let resource = json!({"id":id,"level":quality.as_str(),"dt":view.duration_ms,
            "song":mapping::projected_song_metadata(&view)});
        let existing = queue
            .items
            .iter()
            .find(|e| super::identity::online_song(&e.resource, &id))
            .cloned();
        let mut entry = existing.unwrap_or_else(|| QueueEntry {
            entry_id: uuid::Uuid::new_v4().to_string(),
            resource,
            source: json!({"kind":"songs"}),
        });
        if matches!(origin, NextPlayOrigin::Search) {
            entry.source = search_track_source();
            entry.resource["trial_mode"] = entry.source["trial_mode"].clone();
        }
        let removed: Vec<_> = queue
            .items
            .iter()
            .filter(|e| super::identity::online_song(&e.resource, &id))
            .map(|e| e.entry_id.clone())
            .collect();
        queue.items.retain(|e| !removed.contains(&e.entry_id));
        queue.order.retain(|id| !removed.contains(id));
        let display = queue
            .items
            .iter()
            .position(|e| Some(&e.entry_id) == queue.current.as_ref())
            .map_or(0, |i| i + 1);
        let order = queue
            .order
            .iter()
            .position(|id| Some(id) == queue.current.as_ref())
            .map_or(0, |i| i + 1);
        queue.order.insert(order, entry.entry_id.clone());
        let start = queue.current.is_none();
        if start {
            queue.current = Some(entry.entry_id.clone());
        }
        queue.items.insert(display, entry);
        self.session.check_generation(&token)?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        if start {
            self.claim_native_media(&token, super::super::focus::Owner::Audio, focus)
                .await?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
        }
        let value = controller
            .command(
                &token,
                PlaybackCommand::InsertNext(QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: start.then_some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        self.together_signal(&token, None);
        project(&value, &token)
    }
}

impl MusicClient {
    pub(super) async fn search_group_next_play(
        &self,
        epoch: &str,
        anchor: crate::client::search::SearchDropRow,
        rows: Vec<crate::client::search::SearchDropRow>,
        quality: Quality,
        expected_play_id: String,
        expected_queue_revision: String,
    ) -> Result<PlayerView> {
        use super::queue_playlist::{
            QUEUE_LIMIT, insert_collection_bound, queue_resource_for_track,
        };
        use crate::playback::{PlaybackMode, controller::PlaybackStatus};
        use std::collections::HashSet;

        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let credentials = api
            .search_privilege_credential_revision()?
            .ok_or(Error::StaleResponse)?;
        let pages = self.search.queue_drops.clone();
        let chosen = pages.resolve_group_next(&token, &api, &credentials, &anchor, &rows)?;
        let _room_gate = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let room = self.together_detached_guard(&token);
        let auth = self.auth.pending_guard();
        auth()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if expected_queue_revision != before.queue_revision.to_string()
            || before.play_id.as_deref() != Some(expected_play_id.as_str())
        {
            return Err(Error::QueueChanged);
        }
        // Scope this first lane to the official nonempty queue branch, with a
        // current native decoder and an ordinary, visible, ordered queue.
        if before.startup_pending
            || before.fm
            || queue.intelligence.is_some()
            || queue.mode == PlaybackMode::Random
            || queue.current.is_none()
            || queue.items.iter().any(|item| !item.visible_in_queue())
            || !matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused
            )
            || before.timeline.is_none()
            || room.is_cancelled()
            || self.queue_clear_context(&token)?.room_id.is_some()
        {
            return Err(Error::State("search next requires a stable ordinary queue"));
        }
        let present: HashSet<_> = queue
            .items
            .iter()
            .filter_map(
                |entry| match identity::collection_key(&entry.resource, false) {
                    Some((PlayerResourceKind::Catalog, id)) => Some(id),
                    _ => None,
                },
            )
            .collect();
        let additional = chosen
            .tracks
            .iter()
            .filter(|track| !present.contains(track.id.as_str()))
            .count();
        if queue.items.len().saturating_add(additional) > QUEUE_LIMIT {
            return Err(Error::State("playback queue is full"));
        }
        let source = search_track_source();
        let resources = chosen
            .tracks
            .iter()
            .map(|track| {
                let mut resource = queue_resource_for_track(track, quality);
                resource["trial_mode"] = source["trial_mode"].clone();
                resource
            })
            .collect();
        let next = insert_collection_bound(queue.clone(), resources, source, false, true, false);
        let session = self.session.clone();
        let selection = self.player.selection.clone();
        let attempt = if next == queue {
            selection.load(Ordering::Acquire)
        } else {
            selection.fetch_add(1, Ordering::AcqRel) + 1
        };
        let owner = token.clone();
        let guard = Cancellation::new(move || {
            selection.load(Ordering::Acquire) == attempt
                && auth().is_ok()
                && !room.is_cancelled()
                && session
                    .with_verified_profile(&owner, &api, |_, _| Ok(()))
                    .is_ok()
                && api
                    .search_privilege_credential_revision()
                    .ok()
                    .flatten()
                    .as_ref()
                    == Some(&credentials)
                && pages.group_next_current(&chosen)
        });
        guard.check()?;
        if next == queue {
            return project(&before, &token);
        }
        let expected = before.fence();
        let value = controller
            .command_guarded(
                &token,
                PlaybackCommand::InsertNext(QueueUpdate {
                    expected: expected.clone(),
                    queue: next,
                    transport: None,
                }),
                expected,
                guard,
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}
