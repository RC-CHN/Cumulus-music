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
use std::sync::atomic::Ordering;

pub(super) enum DropSource {
    Local {
        query: LocalQuery,
        revision: String,
        track_id: String,
    },
    Playlist {
        id: String,
        revision: String,
        track_id: String,
        track_index: usize,
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
        let (resource, origin) = match source {
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
                (resource, json!({"kind":"local","revision":revision}))
            }
            DropSource::Playlist {
                id,
                revision,
                track_id,
                track_index,
                quality,
            } => {
                if token.uid.is_none() {
                    return Err(Error::State("playlist drops require an active account"));
                }
                let id = identifier(&json!(id))?;
                let track_id = identifier(&json!(track_id))?;
                let detail = self
                    .session
                    .run_for(&token, |api, _| async {
                        PlaylistApi::new(api).detail(&json!(id)).await
                    })
                    .await?;
                let tracks = detail["trackIds"]
                    .as_array()
                    .ok_or(Error::Response("missing playlist tracks"))?;
                if identifier(&detail["id"])? != id
                    || tracks.len() > 50_000
                    || super::playlist_batch::track_revision(&id, tracks)? != revision
                    || !tracks.get(track_index).is_some_and(|row| {
                        row["v"].as_i64() != Some(-1)
                            && identifier(&row["id"]).is_ok_and(|actual| actual == track_id)
                    })
                {
                    return Err(Error::QueueChanged);
                }
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                let batch = self
                    .session
                    .run_for(&token, |api, _| async {
                        CatalogApi::new(api).songs(&[json!(track_id)], 1).await
                    })
                    .await?;
                let song = batch
                    .songs
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or(Error::State("playlist drop song is unavailable"))?;
                if js_truthy(&song["privilege"]["cs"])
                    || batch
                        .privileges
                        .get(&track_id)
                        .is_some_and(|p| js_truthy(&p["cs"]))
                {
                    return Err(Error::State("playlist drops do not support cloud songs"));
                }
                let resource = super::queue_playlist::catalog_queue_resource(&song, quality)?;
                (resource, json!({"kind":"playlist","id":id}))
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
        insert_drop(&mut queue, resource, origin, boundary)?;
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
    resource: Value,
    source: Value,
    mut boundary: usize,
) -> Result<()> {
    let key = super::identity::collection_key(&resource, false)
        .ok_or(Error::State("drop resource has no queue identity"))?;
    let matches =
        |entry: &QueueEntry| super::identity::collection_key(&entry.resource, false) == Some(key);
    let mut duplicates = queue.items.iter().filter(|entry| matches(entry));
    let first = duplicates.next();
    let current = first
        .filter(|entry| queue.current.as_ref() == Some(&entry.entry_id))
        .or_else(|| duplicates.find(|entry| queue.current.as_ref() == Some(&entry.entry_id)));
    let inserted = if let Some(current) = current {
        // Moving the active duplicate keeps its exact loaded bytes and play ID.
        current.clone()
    } else if let Some(first) = first {
        QueueEntry {
            entry_id: first.entry_id.clone(),
            resource: resource.clone(),
            source,
        }
    } else {
        if queue.items.len() >= super::queue_playlist::QUEUE_LIMIT {
            return Err(Error::State("drop would exceed queue capacity"));
        }
        QueueEntry {
            entry_id: uuid::Uuid::new_v4().to_string(),
            resource: resource.clone(),
            source,
        }
    };
    let mut index = 0;
    let original_boundary = boundary;
    queue.items.retain(|entry| {
        let duplicate = matches(entry);
        if duplicate && index < original_boundary {
            boundary -= 1;
        }
        index += 1;
        !duplicate
    });
    queue.items.insert(boundary, inserted);
    queue.order = queue
        .items
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect();
    Ok(())
}
