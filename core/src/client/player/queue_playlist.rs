//! Official playlist nextPlay/addToPlayList share the current-song insertion
//! point (308.js playing/play); an empty queue starts playback (3085.js).
use super::*;
use crate::{
    catalog::CatalogApi,
    membership::Quality,
    playback::{
        QueueEntry, QueueSnapshot,
        controller::{QueueTransport, QueueUpdate},
    },
    playlist::PlaylistApi,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

// Official module 7.g; insertions keep the nearest prefix, then the suffix.
const QUEUE_LIMIT: usize = 10_000;

pub(super) enum QueueCollection {
    Playlist {
        id: String,
        quality: Quality,
        selection: Option<(Vec<String>, String)>,
    },
    Downloads(Vec<String>),
}

impl MusicClient {
    async fn playlist_queue_resources(
        &self,
        token: &SessionToken,
        id: &str,
        quality: Quality,
        selection: Option<(Vec<String>, String)>,
        attempt: u64,
    ) -> Result<Vec<Value>> {
        let detail = self
            .session
            .run_for(token, |api, _| async {
                PlaylistApi::new(api).detail(&json!(id)).await
            })
            .await?;
        let tracks = detail["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing playlist tracks"))?;
        if tracks.len() > 50_000 {
            return Err(Error::InvalidInput("playlist too large"));
        }
        let mut seen = HashSet::new();
        let ids = if let Some((ids, revision)) = selection {
            super::playlist_batch::selected_ids(id, &detail, &ids, &revision)?
        } else {
            tracks
                .iter()
                .filter(|v| v["v"].as_i64() != Some(-1))
                .map(|v| identifier(&v["id"]))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .map(Value::from)
                .collect::<Vec<_>>()
        };
        let mut resources = Vec::new();
        for chunk in ids.chunks(100) {
            self.session.check_generation(token)?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            let songs = self
                .session
                .run_for(token, |api, _| async {
                    CatalogApi::new(api).songs(chunk, 100).await
                })
                .await?;
            for song in songs.songs.into_iter().flatten() {
                let v = mapping::track(&song)?;
                resources.push(json!({"id":v.id,"level":quality.as_str(),"dt":v.duration_ms,
                    "song":{"id":v.id,"name":v.name,"dt":v.duration_ms,"fee":v.fee,
                    "ar":v.artist_refs.iter().map(|a|json!({"id":a.id,"name":a.name})).collect::<Vec<_>>(),
                    "al":{"id":v.album_id,"name":v.album,"picUrl":v.cover_url}}}));
            }
            if resources.len() >= QUEUE_LIMIT {
                resources.truncate(QUEUE_LIMIT);
                break;
            }
        }
        if resources.is_empty() {
            return Err(Error::State("playlist has no online tracks"));
        }
        Ok(resources)
    }
    pub(super) async fn queue_collection(
        &self,
        epoch: &str,
        collection: QueueCollection,
        expected_revision: String,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        self.session.check_generation(&token)?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision.to_string() != expected_revision {
            return Err(Error::QueueChanged);
        }
        if before.fm {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let (resources, source, downloaded) = match collection {
            QueueCollection::Playlist {
                id,
                quality,
                selection,
            } => {
                self.session.with_token(&token, |_| Ok(()))?;
                let id = identifier(&json!(id))?;
                let resources = self
                    .playlist_queue_resources(&token, &id, quality, selection, attempt)
                    .await?;
                (resources, json!({"kind":"playlist","id":id}), false)
            }
            QueueCollection::Downloads(ids) => (
                self.download_resources(epoch, ids).await?,
                json!({"kind":"downloads"}),
                true,
            ),
        };
        let start = queue.current.is_none();
        let queue = insert_collection(queue, resources, source, downloaded);
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
                PlaybackCommand::UpdateQueue(QueueUpdate {
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

fn resource_key(resource: &Value, downloaded: bool) -> Option<&str> {
    // A local file, an episode and an online song can share the same numeric id.
    if downloaded {
        return resource["download_task_id"].as_str();
    }
    if ["local_id", "voice_id", "download_task_id"]
        .iter()
        .any(|key| resource.get(key).is_some())
    {
        None
    } else {
        resource["id"].as_str()
    }
}

fn insert_collection(
    mut queue: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
    downloaded: bool,
) -> QueueSnapshot {
    let current = queue
        .items
        .iter()
        .find(|e| Some(&e.entry_id) == queue.current.as_ref())
        .cloned();
    let existing: HashMap<_, _> = queue
        .items
        .iter()
        .filter_map(|e| resource_key(&e.resource, downloaded).map(|id| (id, e)))
        .collect();
    let mut seen = HashSet::new();
    let inserted: Vec<_> = resources
        .into_iter()
        .filter_map(|resource| {
            let id = resource_key(&resource, downloaded)?;
            if !seen.insert(id.to_owned())
                || current
                    .as_ref()
                    .is_some_and(|e| resource_key(&e.resource, downloaded) == Some(id))
            {
                return None;
            }
            Some(
                existing
                    .get(id)
                    .map(|e| (*e).clone())
                    .unwrap_or_else(|| QueueEntry {
                        entry_id: uuid::Uuid::new_v4().to_string(),
                        resource,
                        source: source.clone(),
                    }),
            )
        })
        .take(QUEUE_LIMIT - usize::from(current.is_some()))
        .collect();
    // Preserve the loaded entry verbatim, even if the playlist contains it.
    // This avoids restarting or changing the paused position of current audio.
    let replaced: HashSet<_> = inserted
        .iter()
        .filter_map(|e| resource_key(&e.resource, downloaded))
        .collect();
    let split = queue
        .items
        .iter()
        .position(|e| Some(&e.entry_id) == queue.current.as_ref())
        .map_or(0, |i| i + 1);
    let keep = |e: &&QueueEntry| {
        !resource_key(&e.resource, downloaded).is_some_and(|id| replaced.contains(id))
    };
    let space = QUEUE_LIMIT - inserted.len();
    let mut prefix: Vec<_> = queue.items[..split]
        .iter()
        .rev()
        .filter(keep)
        .take(space)
        .cloned()
        .collect();
    prefix.reverse();
    let suffix: Vec<_> = queue.items[split..]
        .iter()
        .filter(keep)
        .take(space - prefix.len())
        .cloned()
        .collect();
    let inserted_ids: HashSet<_> = inserted.iter().map(|e| e.entry_id.clone()).collect();
    let mut items = prefix;
    items.extend(inserted.iter().cloned());
    items.extend(suffix);
    let retained: HashSet<_> = items.iter().map(|e| &e.entry_id).collect();
    queue
        .order
        .retain(|id| retained.contains(id) && !inserted_ids.contains(id));
    let next = queue
        .order
        .iter()
        .position(|id| Some(id) == queue.current.as_ref())
        .map_or(0, |i| i + 1);
    queue
        .order
        .splice(next..next, inserted.into_iter().map(|e| e.entry_id));
    if queue.current.is_none() {
        queue.current = queue.order.first().cloned();
    }
    queue.items = items;
    queue
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_insertions_keep_online_ids_and_distinct_offline_copies_separate() {
        let current = QueueEntry {
            entry_id: "stream".into(),
            resource: json!({"id":"9"}),
            source: Value::Null,
        };
        let before = QueueSnapshot {
            items: vec![current],
            order: vec!["stream".into()],
            current: Some("stream".into()),
            ..Default::default()
        };
        let files = vec![
            json!({"id":"9","download_task_id":"a"}),
            json!({"id":"9","download_task_id":"b"}),
        ];
        let queue = insert_collection(before, files.clone(), json!({"kind":"downloads"}), true);
        let keys = queue.order.clone();
        assert_eq!(queue.items.len(), 3);
        assert_eq!(queue.items[1].resource["download_task_id"], "a");
        assert_eq!(queue.items[2].resource["download_task_id"], "b");
        let again = insert_collection(queue, files, json!({"kind":"downloads"}), true);
        again.validate().unwrap();
        assert_eq!(again.order, keys);
        assert_eq!(again.current.as_deref(), Some("stream"));
    }
    #[test]
    fn capacity_keeps_current_and_nearest_prefix_and_does_not_merge_local_ids() {
        let items: Vec<_> = (0..10_000)
            .map(|i| QueueEntry {
                entry_id: format!("e{i}"),
                resource: json!({"id":i.to_string()}),
                source: Value::Null,
            })
            .collect();
        let mut queue = QueueSnapshot {
            order: items.iter().map(|e| e.entry_id.clone()).collect(),
            current: Some("e9999".into()),
            items,
            ..Default::default()
        };
        queue.items[9998].resource["local_id"] = json!("local-file");
        let value = insert_collection(
            queue,
            vec![
                json!({"id":"9998"}),
                json!({"id":"10000"}),
                json!({"id":"9999"}),
            ],
            Value::Null,
            false,
        );
        value.validate().unwrap();
        assert_eq!(value.items.len(), QUEUE_LIMIT);
        assert_eq!(value.items[0].resource["id"], "2");
        assert_eq!(value.items[9997].entry_id, "e9999");
        assert_eq!(value.items[9996].resource["local_id"], "local-file");
        assert_eq!(value.items[9998].resource["id"], "9998");
        assert_eq!(value.items[9999].resource["id"], "10000");
    }
}
