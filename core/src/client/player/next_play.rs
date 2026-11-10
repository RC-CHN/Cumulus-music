//! Song menu nextPlay: insert after the current entry without replacing playback.
//! Official playingList.addItemToCurPlayingList deduplicates by resource identity.
use super::*;
use crate::{
    catalog::CatalogApi,
    membership::Quality,
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{QueueTransport, QueueUpdate},
    },
    protocol::api::identifier,
};
use rand::Rng;
use serde_json::{Value, json};
use std::{collections::HashSet, sync::atomic::Ordering};

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
        expected_play_id: Option<String>,
        expected_queue_revision: String,
    ) -> Result<PlayerView> {
        use super::{super::focus::Owner, queue_playlist::queue_resource_for_track};

        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let credentials = api
            .search_privilege_credential_revision()?
            .ok_or(Error::StaleResponse)?;
        let pages = self.search.queue_drops.clone();
        let chosen = pages.resolve_group_next(&token, &api, &credentials, &anchor, &rows)?;
        let room = self.together_detached_guard(&token);
        let auth = self.auth.pending_guard();
        auth()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if expected_queue_revision != before.queue_revision.to_string()
            || before.play_id != expected_play_id
        {
            return Err(Error::QueueChanged);
        }
        // Only a genuinely empty ordinary queue can promote Next to Play.
        // Stopped or partially restored queues keep their existing selection.
        let start = queue.items.is_empty()
            && queue.current.is_none()
            && before.play_id.is_none()
            && before.state == PlaybackStatus::Idle;
        let insert = !before.startup_pending
            && queue.current.is_some()
            && before.play_id.is_some()
            && matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused
            )
            && before.timeline.is_some();
        if (!start && !insert)
            || before.fm
            || queue.intelligence.is_some()
            || queue.items.iter().any(|item| !item.visible_in_queue())
            || room.is_cancelled()
            || self.queue_clear_context(&token)?.room_id.is_some()
        {
            return Err(Error::State("search next requires a stable ordinary queue"));
        }
        let focus = start.then(|| self.focus.snapshot()).transpose()?;
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
        let next = search_group_next_queue(
            queue.clone(),
            resources,
            source,
            start.then_some(anchor.track_id.as_str()),
            &mut rand::rng(),
        )?;
        let session = self.session.clone();
        let selection = self.player.selection.clone();
        let attempt = if next == queue {
            selection.load(Ordering::Acquire)
        } else {
            selection.fetch_add(1, Ordering::AcqRel) + 1
        };
        let owner = token.clone();
        let expected_api = api.clone();
        let mut guard = Cancellation::new(move || {
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
        let expected = before.fence();
        let first_source = if let Some(focus) = focus {
            let preparing = {
                let guard = guard.clone();
                let controller = controller.clone();
                let media_focus = self.focus.clone();
                let expected = expected.clone();
                Cancellation::new(move || {
                    !guard.is_cancelled()
                        && controller.snapshot().fence() == expected
                        && media_focus.check_snapshot(focus).is_ok()
                })
            };
            let selected = next.selected().ok_or(Error::QueueChanged)?;
            let first = controller
                .source_resolver()
                .prepare_scoped_search_song(&selected.resource, &token, &expected_api, &preparing)
                .await?;
            preparing.check()?;
            Some(first)
        } else {
            None
        };
        // Do not hold the Together mutation gate while authorizing an empty
        // queue's source. A room transition may supersede that pending read.
        let _room_gate = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        self.validate_queue_clear_room(&token, None)?;
        guard.check()?;
        if next == queue {
            return project(&before, &token);
        }
        if let Some(focus) = focus {
            // Sync any durable queue edit before taking focus from other media.
            let latest = controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?;
            if latest.fence() != expected {
                return Err(Error::QueueChanged);
            }
            let (lease, ()) = self.reserve_prepared_audio(&token, focus, || {
                guard.check()?;
                if controller.snapshot().fence() != expected {
                    return Err(Error::StaleResponse);
                }
                Ok(())
            })?;
            self.finish_media_claim(&token, Owner::Audio, lease).await?;
            let media_focus = self.focus.clone();
            guard = Cancellation::new(move || {
                !guard.is_cancelled() && media_focus.check(lease, Owner::Audio).is_ok()
            });
        }
        let value = controller
            .command_guarded(
                &token,
                PlaybackCommand::ApplySearchGroupNext {
                    update: QueueUpdate {
                        expected: expected.clone(),
                        queue: next,
                        transport: start.then_some(QueueTransport {
                            position: None,
                            paused: false,
                        }),
                    },
                    first_source,
                },
                expected,
                guard,
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}

fn search_group_next_queue(
    queue: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
    start_id: Option<&str>,
    rng: &mut impl Rng,
) -> Result<QueueSnapshot> {
    let incumbent = queue.current.clone();
    let wanted: HashSet<_> = resources
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect();
    let mut next = super::queue_playlist::insert_collection_bound(
        queue, resources, source, false, true, false,
    );
    let inserted: Vec<_> = next
        .items
        .iter()
        .filter(|row| {
            Some(&row.entry_id) != incumbent.as_ref()
                && identity::collection_key(&row.resource, false).is_some_and(|(kind, id)| {
                    kind == PlayerResourceKind::Catalog && wanted.contains(id)
                })
        })
        .collect();
    if let Some(id) = start_id {
        // M3085 sets the clicked song as playId. Neither the selection's
        // first row nor the random traversal may silently replace it.
        next.current = Some(
            inserted
                .iter()
                .find(|row| identity::online_song(&row.resource, id))
                .ok_or(Error::QueueChanged)?
                .entry_id
                .clone(),
        );
    }
    if next.mode == PlaybackMode::Random
        && let Some(first) = inserted.first()
    {
        let batch = inserted.iter().map(|row| row.entry_id.as_str()).collect();
        next.order = super::local_batch::random_order(
            &next,
            &batch,
            &first.entry_id,
            start_id.is_none(),
            rng,
        );
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn full_search_queue_prefers_inserted_rows_then_closest_prefix_and_earliest_suffix() {
        use super::super::queue_playlist::QUEUE_LIMIT;
        for (index, first_old, last_old) in [
            (1, 0, QUEUE_LIMIT - 4),
            (QUEUE_LIMIT - 2, 2, QUEUE_LIMIT - 2),
        ] {
            let items: Vec<_> = (0..QUEUE_LIMIT)
                .map(|i| QueueEntry {
                    entry_id: format!("old-{i}"),
                    resource: json!({"id":(i+1).to_string()}),
                    source: json!({"kind":"songs"}),
                })
                .collect();
            let current = items[index].clone();
            let replaced = items[QUEUE_LIMIT - 1].clone();
            let queue = QueueSnapshot {
                order: items.iter().map(|row| row.entry_id.clone()).collect(),
                items,
                current: Some(current.entry_id.clone()),
                mode: PlaybackMode::Sequential,
                ..Default::default()
            };
            let resources = vec![
                json!({"id":"20001","trial_mode":42}),
                json!({"id":replaced.resource["id"],"trial_mode":42}),
                json!({"id":"20002","trial_mode":42}),
            ];
            let next = search_group_next_queue(
                queue,
                resources,
                search_track_source(),
                None,
                &mut StdRng::seed_from_u64(4),
            )
            .unwrap();
            assert_eq!(next.items.len(), QUEUE_LIMIT);
            assert_eq!(next.selected().as_ref(), Some(&current));
            assert_eq!(
                next.order,
                next.items
                    .iter()
                    .map(|row| row.entry_id.clone())
                    .collect::<Vec<_>>()
            );
            let gap = next
                .items
                .iter()
                .position(|row| row.entry_id == current.entry_id)
                .unwrap()
                + 1;
            assert_eq!(next.items[gap].resource["id"], "20001");
            assert_eq!(next.items[gap + 1].entry_id, replaced.entry_id);
            assert_eq!(next.items[gap + 1].source["scene"], "search");
            assert_eq!(next.items[gap + 2].resource["id"], "20002");
            let retained: Vec<_> = next
                .items
                .iter()
                .filter(|row| row.source["scene"].is_null())
                .map(|row| {
                    row.resource["id"]
                        .as_str()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap()
                        - 1
                })
                .collect();
            assert_eq!(retained, (first_old..=last_old).collect::<Vec<_>>());
            next.validate().unwrap();
        }
    }
}
