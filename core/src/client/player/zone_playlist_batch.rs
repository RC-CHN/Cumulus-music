//! M/3085 and M/1384 retain the queue for zone detail and batch additions.
use super::{
    MusicClient, PlayerView, project,
    queue_playlist::{QUEUE_LIMIT, catalog_queue_resource, insert_zone_batch},
};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    client::{focus::Owner, mapping},
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::atomic::Ordering};

pub(super) struct ZoneQueueSelection {
    pub id: String,
    pub receipt: String,
    pub rows: ZoneQueueRows,
    pub revision: String,
    pub quality: Quality,
}

pub(super) enum ZoneQueueRows {
    Batch(Vec<String>),
    Whole,
    Next(String),
}

fn online_playlist_ids(detail: &Value) -> Result<Vec<Value>> {
    let tracks = detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing zone playlist tracks"))?;
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(tracks.len());
    for track in tracks.iter().filter(|item| item["v"].as_i64() != Some(-1)) {
        let id = identifier(&track["id"])?;
        if seen.insert(id.clone()) {
            ids.push(Value::from(id));
        }
    }
    Ok(ids)
}

impl MusicClient {
    pub(super) async fn zone_playlist_queue_action(
        &self,
        epoch: &str,
        input: ZoneQueueSelection,
        expected_queue_revision: Option<String>,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let id = identifier(&json!(input.id))?;
        let context = self
            .zone_playlist_batch_context(&token, &input.receipt, &id, &input.revision)
            .await?;
        let detail = &context.snapshot.detail;
        let is_batch = matches!(input.rows, ZoneQueueRows::Batch(_));
        let whole = matches!(input.rows, ZoneQueueRows::Whole);
        let mut ids = match input.rows {
            ZoneQueueRows::Batch(ids) => {
                super::playlist_batch::selected_ids(&id, detail, &ids, &input.revision)?
            }
            ZoneQueueRows::Next(song) => {
                super::playlist_batch::selected_ids(&id, detail, &[song], &input.revision)?
            }
            ZoneQueueRows::Whole => online_playlist_ids(detail)?,
        };
        if ids.is_empty() {
            return Err(Error::State("zone playlist has no online songs"));
        }
        if context.guard.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        let play = expected_queue_revision.is_none();
        if expected_queue_revision
            .is_some_and(|expected| before.queue_revision.to_string() != expected)
        {
            return Err(Error::QueueChanged);
        }
        if before.fm || queue.intelligence.is_some() || self.together_attached(&token, None)? {
            return Err(Error::State(
                "zone playlist batch requires ordinary playback",
            ));
        }
        let was_selected = queue.selected();
        let mut limit = ids.len();
        if !is_batch {
            ids.retain(|candidate| {
                !was_selected.as_ref().is_some_and(|entry| {
                    candidate
                        .as_str()
                        .is_some_and(|id| super::identity::online_song(&entry.resource, id))
                })
            });
            if ids.is_empty() {
                return project(&before, &token);
            }
            if whole {
                limit = QUEUE_LIMIT - usize::from(queue.current.is_some());
            }
        }
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let trial = context.snapshot.trial_mode;
        let cached = if is_batch {
            Default::default()
        } else {
            context.snapshot.loaded_queue_tracks()?
        };
        let mut resources = Vec::with_capacity(limit.min(ids.len()));
        let wire_mode = if is_batch { trial } else { -1 };
        let batch_size = if is_batch { 100 } else { 500 };
        for chunk in ids.chunks(batch_size) {
            if context.guard.is_cancelled()
                || self.player.selection.load(Ordering::Acquire) != attempt
            {
                return Err(Error::StaleResponse);
            }
            let missing = chunk
                .iter()
                .filter(|id| !cached.contains_key(id.as_str().unwrap_or_default()))
                .cloned()
                .collect::<Vec<_>>();
            let songs = self
                .session
                .run_for(&token, |api, _| async move {
                    CatalogApi::new(api)
                        .songs_with_trial(&missing, batch_size, Some(wire_mode))
                        .await
                })
                .await?;
            let mut songs = songs.songs.into_iter();
            for id in chunk {
                let mut resource = if let Some(row) = cached.get(id.as_str().unwrap_or_default()) {
                    json!({"id":row.id,"dt":row.duration_ms,"level":input.quality.as_str(),
                        "song":mapping::projected_song_metadata(row)})
                } else if let Some(song) = songs.next().flatten() {
                    catalog_queue_resource(&song, input.quality)?
                } else if whole {
                    // An all-playlist operation, unlike an explicit selection,
                    // may omit a catalog row the service did not return.
                    continue;
                } else {
                    return Err(Error::State("selected zone playlist songs are unavailable"));
                };
                resource["trial_mode"] = json!(trial);
                resources.push(resource);
                if resources.len() == limit {
                    break;
                }
            }
            if resources.len() == limit {
                break;
            }
        }
        let start = play || was_selected.is_none();
        let source = super::origin::named(
            json!({
                "kind":"playlist","id":id,"trial_mode":trial,
                "history_resource":{"id":id,"name":detail["name"],"coverImgUrl":detail["coverImgUrl"],
                    "creator":detail["creator"],"description":detail["description"],"trackCount":detail["trackCount"]}
            }),
            super::origin::resource_name(detail, &id),
        );
        let queue = insert_zone_batch(queue, resources, source, play)?;
        if context.guard.is_cancelled() || self.player.selection.load(Ordering::Acquire) != attempt
        {
            return Err(Error::StaleResponse);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)? {
            return Err(Error::State(
                "zone playlist batch requires ordinary playback",
            ));
        }
        let lease = if start {
            Some(self.claim_native_media(&token, Owner::Audio, focus).await?)
        } else {
            None
        };
        let expected = before.fence();
        let position = (was_selected != queue.selected()).then_some(0.0);
        let command = PlaybackCommand::UpdateQueue(QueueUpdate {
            expected: expected.clone(),
            queue,
            transport: start.then_some(QueueTransport {
                position,
                paused: false,
            }),
        });
        let selection = self.player.selection.clone();
        let audio = self.focus.clone();
        let guard = context.guard;
        let value = controller
            .command_guarded(
                &token,
                command,
                expected,
                Cancellation::new(move || {
                    !guard.is_cancelled()
                        && selection.load(Ordering::Acquire) == attempt
                        && lease.is_none_or(|lease| audio.check(lease, Owner::Audio).is_ok())
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}
