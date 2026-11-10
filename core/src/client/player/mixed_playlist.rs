//! Resolve verified mixed playlist rows without treating device IDs as catalog IDs.
use super::{MixedPlaylistDropRow, MixedPlaylistStart, MusicClient, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::CatalogApi,
    library::LocalResource,
    membership::Quality,
    playback::engine::Cancellation,
    playlist::{PlaylistApi, PlaylistLocalRef, PlaylistLocalRow, PlaylistLocalStore},
    protocol::{api::identifier, wire::serialization::js_truthy},
    storage::OperationLock,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

pub(super) struct MixedRequest<'a> {
    pub id: &'a str,
    pub revision: &'a str,
    pub start: Option<MixedPlaylistStart>,
    pub quality: Quality,
    pub selected_only: bool,
    pub attempt: u64,
}

#[derive(Clone, Copy)]
pub(super) enum MixedInsert<'a> {
    Next(Option<&'a MixedPlaylistStart>),
    Drop(&'a [MixedPlaylistDropRow]),
}

#[derive(Clone, Copy)]
enum Purpose<'a> {
    Play {
        start: Option<&'a MixedPlaylistStart>,
        selected_only: bool,
    },
    Insert(MixedInsert<'a>),
}

impl Purpose<'_> {
    fn is_drop(&self) -> bool {
        matches!(self, Self::Insert(MixedInsert::Drop(_)))
    }

    fn start(&self) -> Option<&MixedPlaylistStart> {
        match self {
            Self::Play { start, .. } => *start,
            Self::Insert(_) => None,
        }
    }

    fn exact(&self) -> bool {
        matches!(
            self,
            Self::Insert(MixedInsert::Drop(_) | MixedInsert::Next(Some(_)))
        )
    }

    fn is_play(&self) -> bool {
        matches!(self, Self::Play { .. })
    }
}

#[derive(Clone)]
enum PlanRow {
    Online(String),
    Local(PlaylistLocalRef),
}

impl PlanRow {
    fn identity(&self) -> (bool, &str) {
        match self {
            Self::Online(id) => (false, id),
            Self::Local(row) => (true, &row.id),
        }
    }
}

pub(super) struct MixedQueue {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub guard: Cancellation,
    // Keep directory membership and hidden state stable until the actor commits.
    pub local_lease: Option<OperationLock>,
}

struct Source<'a> {
    id: &'a str,
    revision: &'a str,
    detail: &'a Value,
    uid: &'a str,
    quality: Quality,
    attempt: u64,
}

impl MusicClient {
    pub(super) async fn mixed_playlist_selection(
        &self,
        token: &SessionToken,
        request: MixedRequest<'_>,
    ) -> Result<MixedQueue> {
        let MixedRequest {
            id,
            revision,
            start,
            quality,
            selected_only,
            attempt,
        } = request;
        self.read_mixed_playlist(
            token,
            id,
            revision,
            quality,
            attempt,
            Purpose::Play {
                start: start.as_ref(),
                selected_only,
            },
        )
        .await
    }

    pub(super) async fn mixed_playlist_insert(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        quality: Quality,
        attempt: u64,
        insert: MixedInsert<'_>,
    ) -> Result<MixedQueue> {
        self.read_mixed_playlist(
            token,
            id,
            revision,
            quality,
            attempt,
            Purpose::Insert(insert),
        )
        .await
    }

    async fn read_mixed_playlist(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        quality: Quality,
        attempt: u64,
        purpose: Purpose<'_>,
    ) -> Result<MixedQueue> {
        let id = identifier(&json!(id))?;
        let (detail, uid) = self
            .session
            .run_for(token, |api, uid| async {
                Ok((PlaylistApi::new(api).detail(&json!(id)).await?, uid))
            })
            .await?;
        self.prepare_mixed_playlist(
            token,
            Source {
                id: &id,
                revision,
                detail: &detail,
                uid: &uid,
                quality,
                attempt,
            },
            purpose,
        )
        .await
    }

    async fn prepare_mixed_playlist(
        &self,
        token: &SessionToken,
        source: Source<'_>,
        purpose: Purpose<'_>,
    ) -> Result<MixedQueue> {
        let Source {
            id,
            revision,
            detail,
            uid,
            quality,
            attempt,
        } = source;
        let playlist = mapping::playlist(detail, uid)?;
        if playlist.id != id || !playlist.owned || playlist.favorite {
            return Err(Error::InvalidInput(
                "mixed playback requires a regular owned playlist",
            ));
        }
        let raw = detail["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing playlist tracks"))?;
        let store = PlaylistLocalStore::new(self.session.clone())?;
        let membership = store.project(token, id, detail)?;
        let changed = || {
            if purpose.is_drop() {
                Error::PlaylistDropSourceChanged
            } else {
                Error::QueueChanged
            }
        };
        if membership.revision != revision {
            return Err(changed());
        }
        if !membership
            .rows
            .iter()
            .any(|row| matches!(row, PlaylistLocalRow::Local(_)))
        {
            return Err(changed());
        }
        if membership.rows.len() > 50_000 {
            return Err(Error::InvalidInput("playlist too large"));
        }
        let total = membership.rows.len();
        let generation = membership.generation();
        let guard = {
            let token = token.clone();
            let id = id.to_owned();
            Cancellation::new(move || {
                store
                    .generation(&token, &id)
                    .is_ok_and(|current| current == generation)
            })
        };
        let plan_row = |row| -> Result<Option<PlanRow>> {
            Ok(match row {
                PlaylistLocalRow::Remote { index } => {
                    let value = raw.get(index).ok_or(Error::QueueChanged)?;
                    if value["v"].as_i64() == Some(-1) {
                        None
                    } else {
                        Some(PlanRow::Online(identifier(&value["id"])?))
                    }
                }
                PlaylistLocalRow::Local(row) => Some(PlanRow::Local(row)),
            })
        };
        let mut plan = Vec::new();
        if let Purpose::Insert(MixedInsert::Drop(rows)) = purpose {
            if rows.is_empty() || rows.len() > super::queue_playlist::QUEUE_LIMIT {
                return Err(Error::InvalidInput(
                    "invalid mixed playlist drop selection size",
                ));
            }
            let mut picked = HashSet::new();
            // Retain the original selection Set order, not the playlist sort.
            for row in rows {
                let value = membership
                    .rows
                    .get(row.track_index)
                    .cloned()
                    .map(plan_row)
                    .transpose()?
                    .flatten();
                let value = value
                    .filter(|value| value.identity() == (row.local, row.track_id.as_str()))
                    .ok_or(Error::InvalidInput(
                        "mixed playlist drop row is outside the verified source",
                    ))?;
                if !picked.insert((row.local, row.track_id.as_str())) {
                    return Err(Error::InvalidInput("duplicate mixed playlist drop row"));
                }
                plan.push(value);
            }
        } else {
            let only = match &purpose {
                Purpose::Play {
                    start,
                    selected_only: true,
                } => *start,
                Purpose::Insert(MixedInsert::Next(track)) => *track,
                _ => None,
            };
            for row in membership.rows {
                if let Some(value) = plan_row(row)?
                    && only.is_none_or(|track| value.identity() == (track.local, track.id.as_str()))
                {
                    plan.push(value);
                }
            }
        }

        let trial = self.ordinary_playlist_trial(token, id, detail)?;
        let mut seen = HashSet::new();
        let online = plan
            .iter()
            .filter_map(|row| match row {
                PlanRow::Online(id) if seen.insert(id.as_str()) => Some(json!(id)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut hydrated = HashMap::new();
        for ids in online.chunks(100) {
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            if guard.is_cancelled() {
                return Err(changed());
            }
            let response = self
                .session
                .run_for(token, |api, _| async {
                    CatalogApi::new(api).songs(ids, 100).await
                })
                .await?;
            if response.songs.len() != ids.len() {
                return Err(Error::Response(
                    "mixed playlist songs changed during hydration",
                ));
            }
            for (id, song) in ids.iter().zip(response.songs) {
                let id = id.as_str().ok_or(Error::QueueChanged)?;
                if purpose.is_drop()
                    && (song
                        .as_ref()
                        .is_some_and(|song| js_truthy(&song["privilege"]["cs"]))
                        || response
                            .privileges
                            .get(id)
                            .is_some_and(|p| js_truthy(&p["cs"])))
                {
                    return Err(Error::State("playlist drops do not support cloud songs"));
                }
                let song = song
                    .as_ref()
                    .map(|song| {
                        mapping::song_metadata_with_privilege(song, response.privileges.get(id))
                    })
                    .transpose()?;
                if song.is_none() && !purpose.is_play() {
                    continue;
                }
                let mut resource = json!({
                    "id": id, "level": quality.as_str(),
                    "dt": song.as_ref().map(|song| &song["dt"]),
                    "missing": song.is_none(), "song": song,
                });
                if let Some(trial) = trial {
                    resource["trial_mode"] = json!(trial);
                }
                hydrated.insert(id.to_owned(), resource);
            }
        }

        let locals = plan
            .iter()
            .filter_map(|row| match row {
                PlanRow::Local(reference) => Some(reference.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (mut items, local_lease) = if locals.is_empty() {
            (Default::default(), None)
        } else {
            let runtime = self.local.context().await?;
            let (items, lease) =
                tokio::task::spawn_blocking(move || runtime.library.hold_playlist_items(&locals))
                    .await
                    .map_err(|_| Error::State("local playlist worker stopped"))??;
            (items, Some(lease))
        };
        let mut resources = Vec::new();
        let mut index = None;
        for row in plan {
            let selected = purpose
                .start()
                .is_none_or(|start| row.identity() == (start.local, start.id.as_str()));
            let resource = match row {
                PlanRow::Online(id) => hydrated.get(&id).cloned(),
                PlanRow::Local(reference) => items
                    .remove(&reference.id)
                    .map(|item| {
                        serde_json::to_value(LocalResource {
                            local_id: item.id,
                            local_revision: item.revision,
                            metadata: item.metadata,
                        })
                    })
                    .transpose()?,
            };
            let Some(resource) = resource else {
                if purpose.exact() {
                    return Err(Error::State("selected playlist song is unavailable"));
                }
                continue;
            };
            if index.is_none() && selected && resource["missing"] != true {
                index = Some(resources.len());
            }
            resources.push(resource);
        }
        let index = index.ok_or(Error::State("selected playlist song is unavailable"))?;
        if guard.is_cancelled() {
            return Err(changed());
        }
        let history = json!({
            "id": id, "name": detail["name"], "coverImgUrl": detail["coverImgUrl"],
            "creator": detail["creator"], "description": detail["description"], "trackCount": total,
        });
        let mut source = super::origin::named(
            json!({"kind":"playlist","id":id,"history_resource":history}),
            super::origin::resource_name(detail, id),
        );
        if let Some(trial) = trial {
            source["trial_mode"] = json!(trial);
        }
        Ok(MixedQueue {
            resources,
            index,
            source,
            guard,
            local_lease,
        })
    }
}
