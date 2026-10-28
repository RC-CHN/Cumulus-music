use super::super::{MusicClient, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::CatalogApi,
    membership::Quality,
    playlist::PlaylistApi,
    podcast::{Episode, PodcastApi, PodcastLibrary, PodcastScope},
    protocol::api::identifier,
    relay::{PullPlan, PullSelection, RelayResource, RelayResourceKind, RelaySourceKind},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::atomic::Ordering};

pub(super) struct Prepared {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
}
impl MusicClient {
    pub(super) async fn prepare_relay(
        &self,
        token: &SessionToken,
        plan: &PullPlan,
        quality: Quality,
        attempt: u64,
    ) -> Result<Prepared> {
        let guard = || {
            self.session.check_generation(token)?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        guard()?;
        let mut episodes = BTreeMap::new();
        let (mut resources, source) = match &plan.selection {
            PullSelection::Resources { resources } => (
                resources.clone(),
                json!({"kind":"relay","scene":"playlistHandoff"}),
            ),
            PullSelection::Collection { source_type, id } => {
                let rows = match source_type {
                    RelaySourceKind::Playlist | RelaySourceKind::Album => {
                        let ids = self
                            .session
                            .run_for(token, |api, _| async {
                                if *source_type == RelaySourceKind::Album {
                                    CatalogApi::new(api)
                                        .album_playback_tracks(&json!(id))
                                        .await?
                                        .iter()
                                        .map(|s| identifier(&s["id"]))
                                        .collect::<Result<Vec<_>>>()
                                } else {
                                    let detail = PlaylistApi::new(api).detail(&json!(id)).await?;
                                    detail["trackIds"]
                                        .as_array()
                                        .ok_or(Error::Response("relay playlist items missing"))?
                                        .iter()
                                        .filter(|s| s["v"].as_i64() != Some(-1))
                                        .map(|s| identifier(&s["id"]))
                                        .collect::<Result<Vec<_>>>()
                                }
                            })
                            .await?;
                        ids.iter()
                            .map(|id| RelayResource::new(&json!(id), RelayResourceKind::Song))
                            .collect::<Result<Vec<_>>>()?
                    }
                    RelaySourceKind::Podcast => {
                        let snapshot = PodcastLibrary::new(self.session.clone())?
                            .read_for(
                                token,
                                &PodcastScope::Radio {
                                    id: id.clone(),
                                    ascending: true,
                                },
                                true,
                            )
                            .await?;
                        let radio = snapshot.radios.into_iter().find(|r| r.id == *id);
                        let mut rows = Vec::new();
                        for mut voice in snapshot.voices {
                            if let Some(radio) = &radio
                                && voice.radio.as_ref().is_none_or(|r| r.id == radio.id)
                            {
                                voice.radio = Some(radio.clone());
                            }
                            rows.push(RelayResource::new(
                                &json!(voice.id),
                                RelayResourceKind::Voice,
                            )?);
                            episodes.insert(voice.id.clone(), voice);
                        }
                        rows
                    }
                };
                (
                    rows,
                    json!({"kind":source_type,"id":id,"scene":"playlistHandoff"}),
                )
            }
        };
        guard()?;
        if let Some(current) = &plan.current
            && !resources.contains(current)
        {
            resources.insert(0, current.clone());
        }
        if resources.is_empty() || resources.len() > 50_000 {
            return Err(Error::Response("invalid relay queue size"));
        }
        let index = plan
            .current
            .as_ref()
            .and_then(|r| resources.iter().position(|item| item == r))
            .unwrap_or(0);
        let mut prepared = BTreeMap::new();
        let songs = resources
            .iter()
            .filter(|r| r.kind == RelayResourceKind::Song)
            .map(|r| r.id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        for chunk in songs.chunks(100) {
            guard()?;
            let (songs, rights) = self
                .session
                .run_for(token, |api, _| async {
                    let api = CatalogApi::new(api);
                    let songs = api
                        .songs(&chunk.iter().map(|id| json!(id)).collect::<Vec<_>>(), 100)
                        .await?;
                    let rights = api.privileges(chunk, None).await?;
                    Ok((songs, rights))
                })
                .await?;
            let rights = rights
                .into_iter()
                .map(|r| Ok((identifier(&r["id"])?, r)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            for (id, song) in chunk.iter().zip(songs.songs) {
                let song = song.as_ref().map(mapping::song_metadata).transpose()?;
                prepared.insert((false,id.clone()),json!({"id":id,"level":quality.as_str(),"dt":song.as_ref().map(|s| &s["dt"]),"song":song,"missing":song.is_none(),"privilege":rights.get(id),"trial_mode":-1}));
            }
        }
        let voices = resources
            .iter()
            .filter(|r| r.kind == RelayResourceKind::Voice)
            .map(|r| r.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let absent = voices
            .iter()
            .filter(|id| !episodes.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        for chunk in absent.chunks(100) {
            guard()?;
            for episode in self
                .session
                .run_for(token, |api, _| async {
                    PodcastApi::new(api).episodes(chunk).await
                })
                .await?
                .into_iter()
                .flatten()
            {
                episodes.insert(episode.id.clone(), episode);
            }
        }
        for id in voices {
            guard()?;
            let Some(mut episode) = episodes.remove(&id) else {
                prepared.insert(
                    (true, id.clone()),
                    json!({"id":id,"voice_id":id,"missing":true}),
                );
                continue;
            };
            if episode.song_id.is_none() {
                let radio = episode.radio.clone();
                episode = self
                    .session
                    .run_for(token, |api, _| async {
                        PodcastApi::new(api).episode(&json!(id)).await
                    })
                    .await?;
                if let Some(radio) = radio
                    && episode.radio.as_ref().is_none_or(|r| r.id == radio.id)
                {
                    episode.radio = Some(radio);
                }
            }
            prepared.insert((true, id), voice_resource(episode)?);
        }
        guard()?;
        let resources = resources
            .into_iter()
            .map(|r| {
                prepared
                    .get(&(r.kind == RelayResourceKind::Voice, r.id))
                    .cloned()
                    .ok_or(Error::Response("relay prepared queue incomplete"))
            })
            .collect::<Result<Vec<_>>>()?;
        if resources[index]["missing"] == true {
            return Err(Error::State("relay current resource unavailable"));
        }
        Ok(Prepared {
            resources,
            index,
            source,
        })
    }
}
fn voice_resource(episode: Episode) -> Result<Value> {
    let mut resource = episode.playback_resource()?;
    resource["song"] = episode.download_metadata()?;
    resource["level"] = json!("standard");
    Ok(resource)
}
