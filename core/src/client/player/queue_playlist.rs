//! Official playlist nextPlay/addToPlayList share the current-song insertion
//! point (308.js playing/play); an empty queue starts playback (3085.js).
use super::{
    super::recommendations::{RecommendationSource, daily_source, daily_subset, style_source},
    *,
};
use crate::{
    catalog::{CatalogApi, details::CatalogQuery},
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
    DailyRecommendations {
        revision: String,
        indices: Option<Vec<usize>>,
        quality: Quality,
    },
    StyleRecommendations {
        revision: String,
        quality: Quality,
    },
    ArtistSongs {
        pages: Vec<super::super::catalog::CatalogSelectionPage>,
        quality: Quality,
    },
    AlbumBatch {
        id: String,
        ids: Vec<String>,
        revision: String,
        quality: Quality,
    },
    Album {
        id: String,
        quality: Quality,
    },
    Playlist {
        id: String,
        quality: Quality,
        selection: Option<(Vec<String>, String)>,
    },
    Downloads(Vec<String>),
    Local {
        query: crate::library::catalog::LocalQuery,
        revision: String,
    },
    Cloud {
        ids: Vec<String>,
        revision: String,
        quality: Quality,
        query: Option<crate::client::dto::CloudQuery>,
    },
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
        self.online_queue_resources(token, &ids, quality, attempt)
            .await
    }
    async fn online_queue_resources(
        &self,
        token: &SessionToken,
        ids: &[Value],
        quality: Quality,
        attempt: u64,
    ) -> Result<Vec<Value>> {
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
        let daily = matches!(collection, QueueCollection::DailyRecommendations { .. });
        let style = matches!(collection, QueueCollection::StyleRecommendations { .. });
        let local = matches!(collection, QueueCollection::Local { .. });
        let artist = matches!(collection, QueueCollection::ArtistSongs { .. });
        let album_batch = matches!(collection, QueueCollection::AlbumBatch { .. });
        let ordinary = daily || style || local || artist || album_batch;
        let special_queue = if daily {
            "daily recommendations require ordinary playback"
        } else if style {
            "style recommendations require ordinary playback"
        } else if artist {
            "artist songs require ordinary playback"
        } else if album_batch {
            "album batch requires ordinary playback"
        } else {
            "local queue requires ordinary playback"
        };
        if ordinary && (queue.intelligence.is_some() || self.together_attached(&token, None)?) {
            return Err(Error::State(special_queue));
        }
        let (recommendation_ids, source_guard) = match &collection {
            QueueCollection::DailyRecommendations {
                revision, indices, ..
            } => {
                let (ids, guard) = self.recommendation_selection(
                    RecommendationSource::Daily,
                    &token,
                    revision,
                    0,
                )?;
                (Some(daily_subset(ids, indices.as_deref())?), Some(guard))
            }
            QueueCollection::StyleRecommendations { revision, .. } => {
                let (ids, guard) = self.recommendation_selection(
                    RecommendationSource::style_for_revision(revision),
                    &token,
                    revision,
                    0,
                )?;
                (Some(ids), Some(guard))
            }
            _ => (None, None),
        };
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let (resources, source, downloaded) = match collection {
            QueueCollection::DailyRecommendations {
                revision, quality, ..
            }
            | QueueCollection::StyleRecommendations { revision, quality } => {
                let mut resources = self
                    .online_queue_resources(
                        &token,
                        &recommendation_ids.ok_or(Error::StaleResponse)?,
                        quality,
                        attempt,
                    )
                    .await?;
                if resources.is_empty() {
                    return Err(Error::State("recommendations have no online tracks"));
                }
                for resource in &mut resources {
                    resource["trial_mode"] = json!(35);
                }
                let source = if style {
                    style_source(&revision)
                } else {
                    daily_source(&revision)
                };
                (resources, source, false)
            }
            QueueCollection::Local {
                mut query,
                revision,
            } => {
                query.revision = Some(revision.clone());
                let context = self.local.context().await?;
                let (resources, _) = tokio::task::spawn_blocking(move || {
                    context.library.catalog(&query)?.queue(None)
                })
                .await
                .map_err(|_| Error::State("local selection worker stopped"))??;
                let already_included = queue.selected().is_some_and(|entry| {
                    let key = resource_key(&entry.resource, false);
                    key.is_some_and(|key| {
                        resources
                            .iter()
                            .any(|item| resource_key(item, false) == Some(key))
                    })
                });
                if resources.len()
                    > QUEUE_LIMIT - usize::from(queue.current.is_some() && !already_included)
                {
                    return Err(Error::State("local queue exceeds insertion limit"));
                }
                (
                    resources,
                    json!({"kind":"local","revision":revision}),
                    false,
                )
            }
            QueueCollection::ArtistSongs { pages, quality } => {
                if pages.is_empty()
                    || pages
                        .iter()
                        .any(|page| !matches!(page.query, CatalogQuery::ArtistSongs { .. }))
                {
                    return Err(Error::InvalidInput("expected artist song pages"));
                }
                let (mut ids, _, source) = self.catalog_song_selection(&token, &pages, 0).await?;
                let mut seen = HashSet::new();
                ids.retain(|id| seen.insert(id.clone()));
                let mut resources = self
                    .online_queue_resources(&token, &ids, quality, attempt)
                    .await?;
                if resources.is_empty() {
                    return Err(Error::State("artist has no online tracks"));
                }
                for resource in &mut resources {
                    resource["trial_mode"] = json!(-1);
                }
                (resources, source, false)
            }
            QueueCollection::AlbumBatch {
                id,
                ids,
                revision,
                quality,
            } => {
                if ids.is_empty() || ids.len() > QUEUE_LIMIT {
                    return Err(Error::InvalidInput("invalid album batch size"));
                }
                let id = identifier(&json!(id))?;
                let mut wanted = ids
                    .iter()
                    .map(|id| identifier(&json!(id)).map(Value::from))
                    .collect::<Result<HashSet<_>>>()?;
                if wanted.len() != ids.len() {
                    return Err(Error::InvalidInput("duplicate album selection"));
                }
                let page = super::super::catalog::CatalogSelectionPage {
                    query: CatalogQuery::Album { id },
                    revision,
                };
                let (loaded, _, source) = self.catalog_song_selection(&token, &[page], 0).await?;
                let selected = loaded
                    .into_iter()
                    .filter(|id| wanted.remove(id))
                    .collect::<Vec<_>>();
                if !wanted.is_empty() {
                    return Err(Error::QueueChanged);
                }
                let mut resources = self
                    .online_queue_resources(&token, &selected, quality, attempt)
                    .await?;
                if resources.len() != selected.len() {
                    return Err(Error::State("selected album songs are unavailable"));
                }
                for resource in &mut resources {
                    resource["trial_mode"] = json!(-1);
                }
                (resources, source, false)
            }
            QueueCollection::Album { id, quality } => {
                self.session.with_token(&token, |_| Ok(()))?;
                let id = identifier(&json!(id))?;
                let tracks = self
                    .session
                    .run_for(&token, |api, _| async {
                        CatalogApi::new(api).album_playback_tracks(&json!(id)).await
                    })
                    .await?;
                if tracks.len() > 50_000 {
                    return Err(Error::InvalidInput("album too large"));
                }
                let mut seen = HashSet::new();
                let ids = tracks
                    .iter()
                    .map(|song| identifier(&song["id"]))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|id| seen.insert(id.clone()))
                    .map(Value::from)
                    .collect::<Vec<_>>();
                let mut resources = self
                    .online_queue_resources(&token, &ids, quality, attempt)
                    .await?;
                if resources.is_empty() {
                    return Err(Error::State("album has no online tracks"));
                }
                for resource in &mut resources {
                    resource["trial_mode"] = json!(-1);
                }
                (
                    resources,
                    json!({"kind":"album","id":id,"scene":"album","trial_mode":-1}),
                    false,
                )
            }
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
                if resources.is_empty() {
                    return Err(Error::State("playlist has no online tracks"));
                }
                (resources, json!({"kind":"playlist","id":id}), false)
            }
            QueueCollection::Downloads(ids) => (
                self.download_resources(epoch, ids).await?,
                json!({"kind":"downloads"}),
                true,
            ),
            QueueCollection::Cloud {
                ids,
                revision,
                quality,
                query,
            } => (
                self.cloud_selected_resources(epoch, ids, &revision, quality, query.clone())
                    .await?,
                json!({"kind":"cloud","revision":revision,"query":query}),
                false,
            ),
        };
        let start = queue.current.is_none();
        let queue = insert_collection(queue, resources, source, downloaded);
        self.session.check_generation(&token)?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        if source_guard
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
        {
            return Err(Error::QueueChanged);
        }
        let _ordinary_room = if ordinary {
            let gate = self
                .together
                .gate
                .try_lock()
                .map_err(|_| Error::MutationBusy)?;
            if self.together_attached(&token, None)? {
                return Err(Error::State(special_queue));
            }
            Some(gate)
        } else {
            None
        };
        if start {
            self.claim_native_media(&token, super::super::focus::Owner::Audio, focus)
                .await?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
        }
        let expected = before.fence();
        let command = PlaybackCommand::UpdateQueue(QueueUpdate {
            expected: expected.clone(),
            queue,
            transport: start.then_some(QueueTransport {
                position: None,
                paused: false,
            }),
        });
        let value = if ordinary {
            let selection = self.player.selection.clone();
            controller
                .command_guarded(
                    &token,
                    command,
                    expected,
                    Cancellation::new(move || {
                        selection.load(Ordering::Acquire) == attempt
                            && source_guard
                                .as_ref()
                                .is_none_or(|guard| !guard.is_cancelled())
                    }),
                )
                .await?
        } else {
            controller.command(&token, command).await?
        };
        self.session.check_generation(&token)?;
        self.together_signal(&token, None);
        project(&value, &token)
    }
}

fn resource_key(resource: &Value, downloaded: bool) -> Option<(PlayerResourceKind, &str)> {
    super::identity::collection_key(resource, downloaded)
}

fn insert_collection(
    mut queue: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
    downloaded: bool,
) -> QueueSnapshot {
    let bind_recommendation_origin = matches!(
        source["kind"].as_str(),
        Some("daily_recommendations" | "style_recommendations")
    );
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
            if !seen.insert((id.0, id.1.to_owned()))
                || current
                    .as_ref()
                    .is_some_and(|e| resource_key(&e.resource, downloaded) == Some(id))
            {
                return None;
            }
            Some(
                existing
                    .get(&id)
                    .map(|e| {
                        let mut entry = (*e).clone();
                        if bind_recommendation_origin
                            || id.0 == PlayerResourceKind::Local && entry.resource != resource
                        {
                            entry.resource = resource.clone();
                            entry.source = source.clone();
                        }
                        entry
                    })
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
        !resource_key(&e.resource, downloaded).is_some_and(|id| replaced.contains(&id))
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
    fn cloud_and_online_entries_with_equal_numeric_ids_remain_distinct() {
        let current = QueueEntry {
            entry_id: "public".into(),
            resource: json!({"id":"9"}),
            source: Value::Null,
        };
        let queue = QueueSnapshot {
            items: vec![current],
            order: vec!["public".into()],
            current: Some("public".into()),
            ..Default::default()
        };
        let cloud = vec![json!({"id":"9","cloud_song_id":"9"})];
        let added = insert_collection(queue, cloud.clone(), json!({"kind":"cloud"}), false);
        assert_eq!(added.items.len(), 2);
        assert_eq!(added.items[1].resource["cloud_song_id"], "9");
        let again = insert_collection(added.clone(), cloud, json!({"kind":"cloud"}), false);
        assert_eq!(again.items.len(), 2);
        assert_eq!(again.order, added.order);
        assert_eq!(again.current.as_deref(), Some("public"));
    }
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
