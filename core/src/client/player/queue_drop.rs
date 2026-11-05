//! Route-bound rows dropped onto a displayed ordinary queue boundary.
use super::*;
use crate::{
    catalog::CatalogApi,
    library::catalog::LocalQuery,
    membership::Quality,
    playback::{PlaybackMode, QueueEntry, QueueSnapshot, controller::QueueUpdate},
    playlist::PlaylistApi,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

use super::{identity::collection_key, queue_playlist::QUEUE_LIMIT};

pub(super) enum DropSource {
    Search {
        rows: Vec<super::super::search::SearchDropRow>,
        quality: Quality,
    },
    Local {
        query: LocalQuery,
        revision: String,
        track_id: String,
    },
    Playlist {
        id: String,
        revision: String,
        rows: Vec<PlaylistDropRow>,
        quality: Quality,
    },
}

pub(super) struct DropTarget {
    pub expected_queue_revision: String,
    pub expected_play_id: Option<String>,
    pub before_entry_id: Option<String>,
}

impl MusicClient {
    pub(super) async fn drop_into_queue(
        &self,
        epoch: &str,
        source: DropSource,
        target: DropTarget,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        self.session.check_generation(&token)?;
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision.to_string() != target.expected_queue_revision
            || before.play_id != target.expected_play_id
        {
            return Err(Error::QueueChanged);
        }
        if before.startup_pending
            || before.fm
            || queue.intelligence.is_some()
            || queue.mode == PlaybackMode::Random
            || queue.items.is_empty()
            || queue.items.iter().any(|item| !item.visible_in_queue())
            || matches!(
                before.state,
                PlaybackStatus::Preparing | PlaybackStatus::Loading
            )
            || matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            ) && before.timeline.is_none()
            || self.together_attached(&token, None)?
        {
            return Err(Error::State(
                "drops require a stable ordinary ordered queue",
            ));
        }
        let boundary = match target.before_entry_id.as_deref() {
            Some(id) => queue
                .items
                .iter()
                .position(|entry| entry.entry_id == id)
                .ok_or(Error::InvalidInput(
                    "drop boundary is outside displayed queue",
                ))?,
            None => queue.items.len(),
        };
        let attempt = self.player.selection.load(Ordering::Acquire);
        let (resources, origin) = match source {
            DropSource::Search { rows, quality } => {
                let rows = self.search_drop_rows(&token, &rows)?;
                let resources = rows
                    .iter()
                    .map(|row| super::queue_playlist::queue_resource_for_track(row, quality))
                    .collect();
                (
                    resources,
                    json!({"kind":"songs","scene":"search","label":"搜索页"}),
                )
            }
            DropSource::Local {
                mut query,
                revision,
                track_id,
            } => {
                query.revision = Some(revision.clone());
                let local = self.local.context().await?;
                let resource = tokio::task::spawn_blocking(move || {
                    let snapshot = local.library.catalog(&query)?;
                    if !snapshot
                        .items
                        .iter()
                        .skip(query.offset)
                        .take(query.limit)
                        .any(|row| row.id == track_id)
                    {
                        return Err(Error::InvalidInput(
                            "local drop row is outside the displayed page",
                        ));
                    }
                    snapshot.queue_track(&track_id)
                })
                .await
                .map_err(|_| Error::State("local drop worker stopped"))??;
                (vec![resource], json!({"kind":"local","revision":revision}))
            }
            DropSource::Playlist {
                id,
                revision,
                rows,
                quality,
            } => {
                if token.uid.is_none() {
                    return Err(Error::State("playlist drops require an active account"));
                }
                if rows.is_empty() || rows.len() > QUEUE_LIMIT {
                    return Err(Error::InvalidInput("invalid playlist drop selection size"));
                }
                let id = identifier(&json!(id))?;
                let detail = self
                    .session
                    .run_for(&token, |api, _| async {
                        PlaylistApi::new(api).detail(&json!(id)).await
                    })
                    .await?;
                let tracks = detail["trackIds"]
                    .as_array()
                    .ok_or(Error::Response("missing playlist tracks"))?;
                if identifier(&detail["id"])? != id {
                    return Err(Error::Response("playlist drop source identity mismatch"));
                }
                if tracks.len() > 50_000 {
                    return Err(Error::InvalidInput(
                        "playlist drop source exceeds the read bound",
                    ));
                }
                if super::playlist_batch::track_revision(&id, tracks)? != revision {
                    return Err(Error::PlaylistDropSourceChanged);
                }
                let mut seen = HashSet::new();
                let mut ids = Vec::new();
                // The ordinary viewer's selection Set is already in gesture
                // order; resolving the source ordinals must not sort it.
                for row in rows {
                    let id = identifier(&json!(row.track_id))?;
                    if !tracks.get(row.track_index).is_some_and(|track| {
                        track["v"].as_i64() != Some(-1)
                            && identifier(&track["id"]).is_ok_and(|actual| actual == id)
                    }) {
                        return Err(Error::InvalidInput(
                            "playlist drop row is outside the verified source",
                        ));
                    }
                    if seen.insert(id.clone()) {
                        ids.push(json!(id));
                    }
                }
                let mut resources = Vec::with_capacity(ids.len());
                for chunk in ids.chunks(100) {
                    if self.player.selection.load(Ordering::Acquire) != attempt {
                        return Err(Error::StaleResponse);
                    }
                    let batch = self
                        .session
                        .run_for(&token, |api, _| async {
                            CatalogApi::new(api).songs(chunk, 100).await
                        })
                        .await?;
                    for (song, id) in batch.songs.into_iter().zip(chunk) {
                        let song = song.ok_or(Error::State("playlist drop song is unavailable"))?;
                        if js_truthy(&song["privilege"]["cs"])
                            || id
                                .as_str()
                                .and_then(|id| batch.privileges.get(id))
                                .is_some_and(|p| js_truthy(&p["cs"]))
                        {
                            return Err(Error::State("playlist drops do not support cloud songs"));
                        }
                        resources.push(super::queue_playlist::catalog_queue_resource(
                            &song, quality,
                        )?);
                    }
                }
                (resources, json!({"kind":"playlist","id":id}))
            }
        };
        self.session.check_generation(&token)?;
        let selection = self.player.selection.clone();
        if selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)?
            || self.queue_clear_context(&token)?.room_id.is_some()
        {
            return Err(Error::State("drops require ordinary playback"));
        }
        insert_drop(&mut queue, &resources, &origin, boundary)?;
        let expected = before.fence();
        let value = controller
            .command_guarded(
                &token,
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: expected.clone(),
                    queue,
                    transport: None,
                }),
                expected,
                Cancellation::new(move || selection.load(Ordering::Acquire) == attempt),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}

fn insert_drop(
    queue: &mut QueueSnapshot,
    resources: &[Value],
    source: &Value,
    boundary: usize,
) -> Result<()> {
    let mut keys = HashSet::new();
    let distinct = resources
        .iter()
        .map(|resource| {
            let key = collection_key(resource, false)
                .ok_or(Error::State("drop resource has no queue identity"))?;
            Ok((key, resource))
        })
        .collect::<Result<Vec<_>>>()?;
    let distinct = distinct
        .into_iter()
        .filter(|(key, _)| keys.insert(*key))
        .collect::<Vec<_>>();
    let mut duplicates = HashMap::new();
    let mut removed = 0;
    let mut prefix_removed = 0;
    for (index, entry) in queue.items.iter().enumerate() {
        if let Some(key) = collection_key(&entry.resource, false).filter(|key| keys.contains(key)) {
            removed += 1;
            prefix_removed += usize::from(index < boundary);
            let first = duplicates.entry(key).or_insert(entry);
            if queue.current.as_ref() == Some(&entry.entry_id) {
                *first = entry;
            }
        }
    }
    if queue.items.len() - removed + distinct.len() > QUEUE_LIMIT {
        return Err(Error::State("drop would exceed queue capacity"));
    }
    let inserted = distinct
        .into_iter()
        .map(|(key, resource)| {
            let prior = duplicates.get(&key);
            if let Some(prior) =
                prior.filter(|entry| queue.current.as_ref() == Some(&entry.entry_id))
            {
                // Moving the active duplicate keeps its exact loaded bytes and play ID.
                return (**prior).clone();
            }
            QueueEntry {
                entry_id: prior.map_or_else(
                    || uuid::Uuid::new_v4().to_string(),
                    |entry| entry.entry_id.clone(),
                ),
                resource: resource.clone(),
                source: source.clone(),
            }
        })
        .collect::<Vec<_>>();
    queue.items.retain(|entry| {
        !collection_key(&entry.resource, false).is_some_and(|key| keys.contains(&key))
    });
    let split = boundary - prefix_removed;
    queue.items.splice(split..split, inserted);
    queue.order = queue
        .items
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect();
    Ok(())
}
