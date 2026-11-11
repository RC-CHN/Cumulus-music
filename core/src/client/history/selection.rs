mod alternatives;

use super::super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::CatalogApi,
    history::{HistoryKind, HistoryRecord, HistoryService},
    membership::Quality,
    podcast::{Episode, PodcastApi, PodcastLibrary, PodcastScope},
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::atomic::Ordering,
};

pub(in crate::client) struct Selection<'a> {
    pub kind: HistoryKind,
    pub revision: &'a str,
    pub rows: SelectionRows<'a>,
    pub index: usize,
    pub quality: Quality,
}

pub(in crate::client) enum SelectionRows<'a> {
    Ids(&'a [String]),
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionMode {
    Direct,
    BatchAdd,
    BatchPlay,
    PlayAll,
}

impl MusicClient {
    /// The WebView supplies only IDs from its displayed native snapshot. File
    /// paths, source metadata, permission requests and queue expansion stay here.
    pub(in crate::client) async fn history_selection(
        &self,
        token: &SessionToken,
        selection: Selection<'_>,
        attempt: u64,
    ) -> Result<(Vec<Value>, usize, Value)> {
        self.history_selection_inner(token, selection, attempt, SelectionMode::Direct)
            .await
    }

    pub(in crate::client) async fn history_selection_for_queue(
        &self,
        token: &SessionToken,
        selection: Selection<'_>,
        attempt: u64,
        play: bool,
    ) -> Result<(Vec<Value>, Value)> {
        if !matches!(selection.kind, HistoryKind::Song | HistoryKind::Voice) {
            return Err(Error::InvalidInput(
                "history batch requires songs or episodes",
            ));
        }
        let mode = match (play, &selection.rows) {
            (true, SelectionRows::All) => SelectionMode::PlayAll,
            (true, SelectionRows::Ids(_)) => SelectionMode::BatchPlay,
            (false, _) => SelectionMode::BatchAdd,
        };
        let (resources, _, source) = self
            .history_selection_inner(token, selection, attempt, mode)
            .await?;
        Ok((resources, source))
    }

    async fn history_selection_inner(
        &self,
        token: &SessionToken,
        selection: Selection<'_>,
        attempt: u64,
        mode: SelectionMode,
    ) -> Result<(Vec<Value>, usize, Value)> {
        let Selection {
            kind,
            revision,
            rows,
            index,
            quality,
        } = selection;
        let guard = || {
            self.session.check_generation(token)?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        guard()?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(token, kind)?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        let chosen = match rows {
            SelectionRows::All => {
                if !matches!(mode, SelectionMode::BatchAdd | SelectionMode::PlayAll)
                    || index != 0
                    || snapshot.entries.len() > 50_300
                {
                    return Err(Error::InvalidInput("invalid full history action"));
                }
                if snapshot.entries.is_empty() {
                    return Err(Error::State("history selection has no playable resources"));
                }
                snapshot.entries
            }
            SelectionRows::Ids(ids) => {
                if ids.is_empty() || ids.len() > 50_300 || index >= ids.len() {
                    return Err(Error::InvalidInput("invalid history selection"));
                }
                let mut previous = None;
                let positions = snapshot
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (r.id.as_str(), i))
                    .collect::<BTreeMap<_, _>>();
                let mut chosen = Vec::with_capacity(ids.len());
                for id in ids {
                    let position = positions
                        .get(id.as_str())
                        .copied()
                        .ok_or(Error::QueueChanged)?;
                    if previous.is_some_and(|p| p >= position) {
                        return Err(Error::InvalidInput("history order changed or duplicated"));
                    }
                    previous = Some(position);
                    chosen.push(snapshot.entries[position].clone());
                }
                chosen
            }
        };
        let (resources, index, source) = match kind {
            HistoryKind::Song => (
                self.history_songs(token, &chosen, quality, &guard, mode)
                    .await?,
                index,
                json!({"kind":"history","history_kind":"song","scene":"history"}),
            ),
            HistoryKind::Voice => {
                self.session.with_token(token, |_| Ok(()))?;
                let voices = chosen
                    .iter()
                    .map(|r| Episode::from_official(&r.resource))
                    .collect::<Result<Vec<_>>>()?;
                // A history row can be selected even when the published
                // episode is gray. Do not expand or resolve it for Play.
                let voices = voices
                    .into_iter()
                    .filter(|episode| {
                        mode != SelectionMode::BatchPlay || episode.raw["grayDisplay"] != true
                    })
                    .collect();
                (
                    self.history_voices(token, voices, &guard).await?,
                    index,
                    json!({"kind":"history","history_kind":"voice","scene":"history"}),
                )
            }
            _ => {
                if chosen.len() != 1 || index != 0 {
                    return Err(Error::InvalidInput("select one history collection"));
                }
                self.session.with_token(token, |_| Ok(()))?;
                let row = &chosen[0];
                match kind {
                    HistoryKind::Album | HistoryKind::Playlist => {
                        let songs = self
                            .session
                            .run_for(token, |api, _| async {
                                if kind == HistoryKind::Album {
                                    CatalogApi::new(api)
                                        .album_playback_tracks(&json!(row.id))
                                        .await?
                                        .iter()
                                        .map(|song| identifier(&song["id"]))
                                        .collect::<Result<Vec<_>>>()
                                } else {
                                    let page = crate::playlist::PlaylistApi::new(api)
                                        .detail(&json!(row.id))
                                        .await?;
                                    page["trackIds"]
                                        .as_array()
                                        .ok_or(Error::Response("history playlist tracks missing"))?
                                        .iter()
                                        .filter(|v| v["v"].as_i64() != Some(-1))
                                        .map(|v| identifier(&v["id"]))
                                        .collect::<Result<Vec<_>>>()
                                }
                            })
                            .await?;
                        if songs.is_empty() || songs.len() > 50_000 {
                            return Err(Error::State("history collection has no playable queue"));
                        }
                        guard()?;
                        let records = songs
                            .into_iter()
                            .map(|id| HistoryRecord::local(HistoryKind::Song, json!({"id":id}), 0))
                            .collect::<Result<Vec<_>>>()?;
                        let source = json!({"kind":if kind==HistoryKind::Album{"album"}else{"playlist"},"id":row.id,
                            "scene":"history","history_resource":row.resource});
                        (
                            self.history_songs(
                                token,
                                &records,
                                quality,
                                &guard,
                                SelectionMode::Direct,
                            )
                            .await?,
                            0,
                            source,
                        )
                    }
                    HistoryKind::Podcast | HistoryKind::Audiobook => {
                        let snapshot = PodcastLibrary::new(self.session.clone())?
                            .read_for(
                                token,
                                &PodcastScope::Radio {
                                    id: row.id.clone(),
                                    ascending: true,
                                },
                                true,
                            )
                            .await?;
                        let mut voices = snapshot.voices;
                        if voices.is_empty() {
                            return Err(Error::State("history program collection is empty"));
                        }
                        // Complete radio metadata supplies audiobook categories omitted
                        // from individual program rows, without replacing program IDs.
                        if let Some(radio) = snapshot.radios.into_iter().find(|r| r.id == row.id) {
                            for voice in &mut voices {
                                if voice.radio.as_ref().is_none_or(|r| r.id == radio.id) {
                                    voice.radio = Some(radio.clone());
                                }
                            }
                        }
                        (
                            self.history_voices(token, voices, &guard).await?,
                            0,
                            json!({"kind":"history","history_kind":kind,"scene":"history"}),
                        )
                    }
                    _ => unreachable!("handled row kinds"),
                }
            }
        };
        guard()?;
        // Loading metadata is read-only; a concurrently changed displayed list
        // cannot silently become the source of a new queue.
        if history.snapshot_for(token, kind)?.revision != revision {
            return Err(Error::QueueChanged);
        }
        if mode == SelectionMode::Direct
            && resources.get(index).is_none_or(|r| r["missing"] == true)
        {
            return Err(Error::State("selected history resource is unavailable"));
        }
        Ok((resources, index, source))
    }

    async fn history_songs(
        &self,
        token: &SessionToken,
        rows: &[HistoryRecord],
        quality: Quality,
        guard: &(impl Fn() -> Result<()> + Sync),
        mode: SelectionMode,
    ) -> Result<Vec<Value>> {
        let offline = (mode != SelectionMode::Direct)
            .then(|| crate::download::offline::OfflineLibrary::new(self.session.clone()))
            .transpose()?;
        let mut resources = Vec::with_capacity(rows.len());
        let mut online = Vec::new();
        let mut alternatives = Vec::new();
        for row in rows {
            guard()?;
            if let Some(id) = row.resource["local_id"].as_str() {
                let context = self.local.context().await?;
                let id = id.to_owned();
                let expected = row.resource["local_revision"].clone();
                let resource = tokio::task::spawn_blocking(move || -> Result<Value> {
                    let resource = context.library.resource(&id)?;
                    if serde_json::to_value(&resource.local_revision)? != expected {
                        return Err(Error::State(
                            "local history file changed; select it from the library",
                        ));
                    }
                    Ok(serde_json::to_value(resource)?)
                })
                .await
                .map_err(|_| Error::State("local history selection stopped"))?;
                guard()?;
                match resource {
                    Ok(resource) => resources.push(resource),
                    Err(error)
                        if mode == SelectionMode::PlayAll && unavailable_local_row(&error) =>
                    {
                        resources.push(json!({"missing":true}));
                    }
                    Err(error) => return Err(error),
                }
            } else {
                self.session.with_token(token, |_| Ok(()))?;
                // Use a verified completed download when it still exists. A
                // failed offline check may fall back only to fresh online rights.
                if let Some(task) = row.resource["download_task_id"].as_str()
                    && let Ok(mut download) = self
                        .download_resources(&token.generation.to_string(), vec![task.into()])
                        .await
                {
                    guard()?;
                    if !download.is_empty() {
                        let publication = download.remove(0);
                        let mut eligible = true;
                        if let Some(offline) = &offline {
                            eligible = if let Some(path) = publication["path"].as_str() {
                                offline.check(token, path, false).await.is_ok_and(|check| {
                                    check.access.can_play
                                        && check.integrity_verified
                                        && check.record.task_id == task
                                        && check.record.song_id == row.id
                                })
                            } else {
                                false
                            };
                            guard()?;
                        }
                        if eligible {
                            resources.push(publication);
                            continue;
                        }
                    }
                }
                online.push((resources.len(), row.id.clone()));
                resources.push(Value::Null);
            }
        }
        for chunk in online.chunks(100) {
            guard()?;
            let ids = chunk.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>();
            let (songs, privileges) = self
                .session
                .run_for(token, |api, _| async {
                    let api = CatalogApi::new(api);
                    let songs = api
                        .songs(&ids.iter().map(|id| json!(id)).collect::<Vec<_>>(), 100)
                        .await?;
                    let privileges = api.privileges(&ids, None).await?;
                    Ok((songs, privileges))
                })
                .await?;
            guard()?;
            let privileges = privileges
                .into_iter()
                .map(|p| Ok((identifier(&p["id"])?, p)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            for ((slot, id), song) in chunk.iter().zip(songs.songs) {
                let alternate = (mode == SelectionMode::PlayAll)
                    .then(|| alternatives::declared(id, song.as_ref(), privileges.get(id)))
                    .flatten();
                let song = song
                    .as_ref()
                    .map(super::super::mapping::song_metadata)
                    .transpose()?;
                resources[*slot] = json!({"id":id,"level":quality.as_str(),"dt":song.as_ref().map(|s|&s["dt"]),
                    "missing":song.is_none(),"song":song,"privilege":privileges.get(id),"trial_mode":-1});
                if let Some(id) = alternate {
                    alternatives.push((*slot, id));
                }
            }
        }
        if !alternatives.is_empty() {
            let ids: Vec<_> = alternatives.iter().map(|(_, id)| id.clone()).collect();
            let replacements = self
                .session
                .run_for(token, |api, _| alternatives::resolve(api, &ids, quality))
                .await?;
            guard()?;
            for (slot, id) in alternatives {
                if let Some(resource) = replacements.get(&id) {
                    resources[slot] = resource.clone();
                }
            }
        }
        Ok(resources)
    }
    async fn history_voices(
        &self,
        token: &SessionToken,
        voices: Vec<Episode>,
        guard: &(impl Fn() -> Result<()> + Sync),
    ) -> Result<Vec<Value>> {
        if voices.len() > 50_000 {
            return Err(Error::InvalidInput("history voice queue too large"));
        }
        let mut resources = Vec::with_capacity(voices.len());
        let mut ids = HashSet::new();
        for mut episode in voices {
            guard()?;
            if !ids.insert(episode.id.clone()) {
                return Err(Error::Response("duplicate history program"));
            }
            if episode.song_id.is_none() {
                let radio = episode.radio.clone();
                episode = self
                    .session
                    .run_for(token, |api, _| async {
                        PodcastApi::new(api).episode(&json!(episode.id)).await
                    })
                    .await?;
                if let Some(radio) = radio
                    && episode.radio.as_ref().is_none_or(|r| r.id == radio.id)
                {
                    episode.radio = Some(radio);
                }
            }
            let mut resource = episode.playback_resource()?;
            resource["song"] = episode.download_metadata()?;
            resource["level"] = json!("standard");
            resources.push(resource);
        }
        guard()?;
        Ok(resources)
    }
}

fn unavailable_local_row(error: &Error) -> bool {
    matches!(
        error,
        Error::State(
            "local track is not available"
                | "local track is unavailable"
                | "local source changed; rescan before playing"
                | "local history file changed; select it from the library"
        )
    ) || matches!(error, Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
}
