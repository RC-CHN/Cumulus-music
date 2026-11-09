//! Queue a verified mixed playlist without treating device IDs as catalog IDs.
use super::{MixedPlaylistStart, MusicClient, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::CatalogApi,
    library::LocalResource,
    membership::Quality,
    playback::engine::Cancellation,
    playlist::{PlaylistApi, PlaylistLocalRow, PlaylistLocalStore},
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub(super) struct MixedRequest<'a> {
    pub id: &'a str,
    pub revision: &'a str,
    pub start: Option<MixedPlaylistStart>,
    pub quality: Quality,
    pub selected_only: bool,
    pub attempt: u64,
}

pub(super) struct MixedQueue {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub guard: Cancellation,
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
        let id = identifier(&json!(id))?;
        let (detail, uid) = self
            .session
            .run_for(token, |api, uid| async {
                Ok((PlaylistApi::new(api).detail(&json!(id)).await?, uid))
            })
            .await?;
        let playlist = mapping::playlist(&detail, &uid)?;
        if playlist.id != id || !playlist.owned || playlist.favorite {
            return Err(Error::InvalidInput(
                "mixed playback requires a regular owned playlist",
            ));
        }
        let raw = detail["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing playlist tracks"))?;
        let store = PlaylistLocalStore::new(self.session.clone())?;
        let membership = store.project(token, &id, &detail)?;
        if membership.revision != revision
            || !membership
                .rows
                .iter()
                .any(|row| matches!(row, PlaylistLocalRow::Local(_)))
        {
            return Err(Error::QueueChanged);
        }
        if membership.rows.len() > 50_000 {
            return Err(Error::InvalidInput("playlist too large"));
        }
        let total = membership.rows.len();
        let generation = membership.generation();
        let guard = {
            let token = token.clone();
            let id = id.clone();
            Cancellation::new(move || {
                store
                    .generation(&token, &id)
                    .is_ok_and(|current| current == generation)
            })
        };
        let matches_start = |local, id: &str| {
            start
                .as_ref()
                .is_some_and(|target| target.local == local && target.id == id)
        };
        let online_id = |index| -> Result<Option<String>> {
            let value: &Value = raw.get(index).ok_or(Error::QueueChanged)?;
            if value["v"].as_i64() == Some(-1) {
                Ok(None)
            } else {
                identifier(&value["id"]).map(Some)
            }
        };
        let mut online = Vec::new();
        let mut locals = Vec::new();
        for row in &membership.rows {
            match row {
                PlaylistLocalRow::Remote { index } => {
                    if let Some(id) = online_id(*index)?
                        && (!selected_only || matches_start(false, &id))
                    {
                        online.push(Value::String(id));
                    }
                }
                PlaylistLocalRow::Local(reference) => {
                    if !selected_only || matches_start(true, &reference.id) {
                        locals.push(reference.clone());
                    }
                }
            }
        }

        let trial = self.ordinary_playlist_trial(token, &id, &detail)?;
        let mut hydrated = Vec::with_capacity(online.len());
        for ids in online.chunks(100) {
            if self.player.selection.load(Ordering::Acquire) != attempt || guard.is_cancelled() {
                return Err(Error::StaleResponse);
            }
            let response = self
                .session
                .run_for(token, |api, _| async {
                    CatalogApi::new(api).songs_with_trial(ids, 100, None).await
                })
                .await?;
            if response.songs.len() != ids.len() {
                return Err(Error::Response(
                    "mixed playlist songs changed during hydration",
                ));
            }
            for (id, song) in ids.iter().zip(response.songs) {
                let song = song.as_ref().map(mapping::song_metadata).transpose()?;
                let mut resource = json!({
                    "id": id, "level": quality.as_str(),
                    "dt": song.as_ref().map(|song| &song["dt"]),
                    "missing": song.is_none(), "song": song,
                });
                if let Some(trial) = trial {
                    resource["trial_mode"] = json!(trial);
                }
                hydrated.push(resource);
            }
        }

        let mut items = if locals.is_empty() {
            Default::default()
        } else {
            let runtime = self.local.context().await?;
            tokio::task::spawn_blocking(move || runtime.library.playlist_local_items(&locals))
                .await
                .map_err(|_| Error::State("local playlist worker stopped"))??
        };
        let mut hydrated = hydrated.into_iter();
        let mut resources = Vec::new();
        let mut index = None;
        for row in membership.rows {
            let (resource, selected) = match row {
                PlaylistLocalRow::Remote { index } => {
                    let Some(id) = online_id(index)? else {
                        continue;
                    };
                    let selected = matches_start(false, &id);
                    if selected_only && !selected {
                        continue;
                    }
                    (hydrated.next().ok_or(Error::QueueChanged)?, selected)
                }
                PlaylistLocalRow::Local(reference) => {
                    let Some(item) = items.remove(&reference.id) else {
                        continue;
                    };
                    let selected = matches_start(true, &item.id);
                    (
                        serde_json::to_value(LocalResource {
                            local_id: item.id,
                            local_revision: item.revision,
                            metadata: item.metadata,
                        })?,
                        selected,
                    )
                }
            };
            if index.is_none() && (selected || start.is_none()) && resource["missing"] != true {
                index = Some(resources.len());
            }
            resources.push(resource);
        }
        let index = index.ok_or(Error::State("selected playlist song is unavailable"))?;
        if guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let history = json!({
            "id": id, "name": detail["name"], "coverImgUrl": detail["coverImgUrl"],
            "creator": detail["creator"], "description": detail["description"],
            "trackCount": total,
        });
        let mut source = super::origin::named(
            json!({"kind":"playlist","id":id,"history_resource":history}),
            super::origin::resource_name(&detail, &id),
        );
        if let Some(trial) = trial {
            source["trial_mode"] = json!(trial);
        }
        Ok(MixedQueue {
            resources,
            index,
            source,
            guard,
        })
    }
}
