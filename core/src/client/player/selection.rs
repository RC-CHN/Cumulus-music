use super::*;
use crate::{catalog::CatalogApi, playlist::PlaylistApi, protocol::api::identifier};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

impl MusicClient {
    pub async fn play_downloads(
        &self,
        epoch: &str,
        task_ids: Vec<String>,
        index: usize,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        if index >= task_ids.len() {
            return Err(Error::InvalidInput("invalid download selection index"));
        }
        let lease = self
            .claim_media(&token, super::super::focus::Owner::Audio)
            .await?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let resources = self.download_resources(epoch, task_ids).await?;
        let controller = self.player.controller().await?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        self.focus.check(lease, super::super::focus::Owner::Audio)?;
        let value = controller
            .command(
                &token,
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source: json!({"kind":"downloads"}),
                },
            )
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        project(&value, &token)
    }
    pub async fn play_selection(&self, epoch: &str, input: PlaySelection) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        if !matches!(
            input,
            PlaySelection::Local { .. }
                | PlaySelection::History { .. }
                | PlaySelection::Downloads { .. }
        ) {
            self.session.with_token(&token, |_| Ok(()))?;
        }
        let lease = self
            .claim_media(&token, super::super::focus::Owner::Audio)
            .await?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let input = match input {
            PlaySelection::Downloads { query, start_id } => {
                let (resources, index) = self.download_selection(epoch, query, start_id).await?;
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let state = controller
                    .command(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"downloads"}),
                        },
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::History {
                history_kind,
                revision,
                ids,
                index,
                quality,
            } => {
                let (resources, index, source) = self
                    .history_selection(
                        &token,
                        super::super::history::Selection {
                            kind: history_kind,
                            revision: &revision,
                            ids: &ids,
                            index,
                            quality,
                        },
                        attempt,
                    )
                    .await?;
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let state = controller
                    .command(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source,
                        },
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::Podcast {
                scope,
                start_id,
                revision,
            } => {
                let (resources, index) = self
                    .podcast_resources(epoch, &scope, &revision, start_id.as_deref())
                    .await?;
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let state = controller
                    .command(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"podcast","scope":scope,"revision":revision}),
                        },
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            PlaySelection::Local {
                mut query,
                start_id,
                revision,
            } => {
                query.revision = Some(revision.clone());
                let context = self.local.context().await?;
                let (resources, index) = tokio::task::spawn_blocking(move || {
                    context.library.catalog(&query)?.queue(start_id.as_deref())
                })
                .await
                .map_err(|_| Error::State("local selection worker stopped"))??;
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let state = controller
                    .command(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"local","revision":revision}),
                        },
                    )
                    .await?;
                self.session.check_generation(&token)?;
                return project(&state, &token);
            }
            PlaySelection::MvRelated {
                mv_id,
                song_id,
                quality,
            } => {
                let song_id = identifier(&json!(song_id))?;
                let detail = crate::mv::MvService::new(self.session.clone())?
                    .detail_for(&token, &json!(mv_id))
                    .await?;
                if !detail
                    .related_songs
                    .iter()
                    .any(|s| identifier(&s["id"]).ok().as_deref() == Some(song_id.as_str()))
                {
                    return Err(Error::InvalidInput("song is not related to this MV"));
                }
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let current = controller.snapshot();
                if current.account == token
                    && current
                        .selected
                        .as_ref()
                        .is_some_and(|s| s.resource["id"].as_str() == Some(&song_id))
                {
                    let value = controller.command(&token, PlaybackCommand::Play).await?;
                    self.session.with_token(&token, |_| Ok(()))?;
                    return project(&value, &token);
                }
                PlaySelection::MvRelated {
                    mv_id: detail.id,
                    song_id,
                    quality,
                }
            }
            PlaySelection::Cloud {
                query,
                start_id,
                revision,
                quality,
            } => {
                let (resources, index) = self
                    .cloud_resources(epoch, query, &revision, start_id, quality)
                    .await?;
                let controller = self.player.controller().await?;
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, super::super::focus::Owner::Audio)?;
                let state = controller
                    .command(
                        &token,
                        PlaybackCommand::Replace {
                            resources,
                            index,
                            source: json!({"kind":"cloud","revision":revision}),
                        },
                    )
                    .await?;
                self.session.with_token(&token, |_| Ok(()))?;
                return project(&state, &token);
            }
            other => other,
        };
        let selection_token = token.clone();
        let (ids, index, quality, source) = self
            .session
            .run_for(&token, |api, _| async move {
                match input {
                    PlaySelection::Catalog {
                        pages,
                        index,
                        quality,
                    } => {
                        let (ids, index, source) = self
                            .catalog_song_selection(&selection_token, &pages, index)
                            .await?;
                        Ok((ids, index, quality, source))
                    }
                    PlaySelection::Member {
                        query,
                        revision,
                        section,
                        index,
                        quality,
                    } => {
                        self.member_song_selection(
                            epoch,
                            query,
                            &revision,
                            section.as_deref(),
                            index,
                            quality,
                        )
                        .await
                    }
                    PlaySelection::Album {
                        id,
                        start_id,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let detail = CatalogApi::new(api).album(&json!(id)).await?;
                        let songs = detail["songs"]
                            .as_array()
                            .ok_or(Error::Response("missing album songs"))?;
                        let (ids, actual) = super::super::rankings::album_order(&id, songs)?;
                        if revision != actual {
                            return Err(Error::QueueChanged);
                        }
                        let index = match start_id {
                            Some(id) => ids
                                .iter()
                                .position(|v| v.as_str() == Some(&id))
                                .ok_or(Error::InvalidInput("song not in album"))?,
                            None => 0,
                        };
                        if ids.is_empty() {
                            return Err(Error::State("album has no songs"));
                        }
                        Ok((ids, index, quality, json!({"kind":"album","id":id})))
                    }
                    PlaySelection::MvRelated {
                        mv_id,
                        song_id,
                        quality,
                    } => Ok((
                        vec![json!(song_id)],
                        0,
                        quality,
                        json!({"kind":"mv_related","mvId":mv_id,"scene":"video"}),
                    )),
                    PlaySelection::Cloud { .. }
                    | PlaySelection::Downloads { .. }
                    | PlaySelection::History { .. }
                    | PlaySelection::Local { .. }
                    | PlaySelection::Podcast { .. } => {
                        unreachable!("cloud handled before song detail requests")
                    }
                    PlaySelection::Songs {
                        ids,
                        index,
                        quality,
                    } => {
                        if ids.is_empty() || ids.len() > 500 || index >= ids.len() {
                            return Err(Error::InvalidInput("invalid song selection"));
                        }
                        let ids = ids
                            .iter()
                            .map(|id| identifier(&json!(id)).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        Ok((ids, index, quality, json!({"kind":"songs"})))
                    }
                    PlaySelection::PlaylistBatch {
                        id,
                        ids,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let detail = PlaylistApi::new(api).detail(&json!(id)).await?;
                        let ids =
                            super::playlist_batch::selected_ids(&id, &detail, &ids, &revision)?;
                        Ok((ids, 0, quality, json!({"kind":"playlist","id":id})))
                    }
                    PlaySelection::Playlist {
                        id,
                        start_id,
                        revision,
                        quality,
                    } => {
                        let id = identifier(&json!(id))?;
                        let detail = PlaylistApi::new(api).detail(&json!(id)).await?;
                        let tracks = detail["trackIds"]
                            .as_array()
                            .ok_or(Error::Response("missing playlist tracks"))?;
                        let actual =
                            hex::encode(Sha256::digest(serde_json::to_vec(&json!([id, tracks]))?));
                        if revision.is_some_and(|r| r != actual) {
                            return Err(Error::QueueChanged);
                        }
                        if tracks.len() > 50000 {
                            return Err(Error::InvalidInput("playlist too large"));
                        }
                        let ids = tracks
                            .iter()
                            .filter(|v| v["v"].as_i64() != Some(-1))
                            .map(|v| identifier(&v["id"]).map(Value::from))
                            .collect::<Result<Vec<_>>>()?;
                        let index = match start_id {
                            Some(id) => ids
                                .iter()
                                .position(|v| v.as_str() == Some(&id))
                                .ok_or(Error::InvalidInput("song not in playlist"))?,
                            None => 0,
                        };
                        if ids.is_empty() {
                            return Err(Error::State("playlist has no online tracks"));
                        }
                        let history = json!({"id":id,"name":detail["name"],
                            "coverImgUrl":detail["coverImgUrl"],"creator":detail["creator"],
                            "description":detail["description"],"trackCount":detail["trackCount"]});
                        Ok((
                            ids,
                            index,
                            quality,
                            json!({"kind":"playlist","id":id,"history_resource":history}),
                        ))
                    }
                }
            })
            .await?;
        let mut resources = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(100) {
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            let songs = self
                .session
                .run_for(&token, |api, _| async {
                    CatalogApi::new(api).songs(chunk, 100).await
                })
                .await?;
            for (id, song) in chunk.iter().zip(songs.songs) {
                let song= song.map(|song| {
                    let view=mapping::track(&song)?;
                    Ok::<_,Error>(json!({"id":view.id,"name":view.name,"ar":view.artist_refs.iter().map(|artist|json!({"id":artist.id,"name":artist.name})).collect::<Vec<_>>(),
                        "al":{"id":view.album_id,"name":view.album,"picUrl":view.cover_url},"dt":view.duration_ms,"fee":view.fee}))
                }).transpose()?;
                let mut resource = json!({"id":id,"level":quality.as_str(),"dt":song.as_ref().map(|s|&s["dt"]),"missing":song.is_none(),"song":song});
                if let Some(trial) = source.get("trial_mode") {
                    resource["trial_mode"] = trial.clone();
                }
                resources.push(resource);
            }
        }
        if resources.get(index).is_none_or(|v| v["missing"] == true) {
            return Err(Error::State("selected song is unavailable"));
        }
        let controller = self.player.controller().await?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        self.focus.check(lease, super::super::focus::Owner::Audio)?;
        let state = controller
            .command(
                &token,
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source,
                },
            )
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        project(&state, &token)
    }
}
