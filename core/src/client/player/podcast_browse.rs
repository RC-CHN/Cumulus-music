//! M3085 needFetchAudio for the current weekly and voice-rank Play buttons.
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, prepared_selection::PreparedSelection,
    queue_playlist::insert_collection_bound, voice_detail::bound_episode,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::podcast::{
        browse_playback::Target,
        radio_playback::{around_request, order_around_programs, parse_around_programs},
    },
    playback::{
        PlaybackMode, QueueSnapshot,
        controller::{QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    podcast::{Episode, PodcastApi, PodcastLibrary, progress},
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

fn request_failure(error: &Error) -> bool {
    match error {
        Error::Transport(_)
        | Error::HttpStatus {
            status: 500..=599, ..
        } => true,
        Error::Service { code, .. } => {
            code != 301
                && code != "301"
                && code != 401
                && code != "401"
                && code != 403
                && code != "403"
        }
        // Authentication, pending transitions, and invalid reply identities
        // are not authority for a second network request.
        _ => false,
    }
}

fn compatible(target: &Target, episode: &Episode) -> bool {
    episode.id == target.id
        && target
            .song
            .as_ref()
            .is_none_or(|expected| episode.song_id.as_ref().is_none_or(|song| song == expected))
        && target.radio.as_ref().is_none_or(|expected| {
            episode
                .radio
                .as_ref()
                .is_none_or(|radio| &radio.id == expected)
        })
}

pub(super) fn resource(voice: &Episode) -> Result<Value> {
    let mut resource = voice.playback_resource()?;
    resource["song"] = voice.download_metadata()?;
    resource["level"] = json!("standard");
    if bound_episode(&resource).is_none() {
        return Err(Error::Response(
            "podcast browse program media identity mismatch",
        ));
    }
    Ok(resource)
}

pub(super) fn voice_replacement_mode(category: &Value, old_mode: PlaybackMode) -> PlaybackMode {
    if matches!(category.as_i64(), Some(3001 | 10002 | 2001)) {
        old_mode
    } else {
        PlaybackMode::Sequential
    }
}

impl MusicClient {
    pub(super) async fn podcast_browse_selection(
        &self,
        token: &SessionToken,
        receipt: &str,
        double_click: bool,
        prepared: &PreparedSelection,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        let api = self.session.native_api(token)?;
        let (observation, target) = self.podcast_browse_playback.resolve(token, &api, receipt)?;
        if double_click && !observation.lane.is_full_rank() {
            return Err(Error::InvalidInput(
                "double click requires a full voice rank",
            ));
        }
        let replace = double_click && self.replace_queue_on_double_click(token)?;
        let chart = replace
            .then(|| self.podcast_browse_playback.full_rank(&observation))
            .transpose()?;
        let clicked_chart_row = chart
            .as_ref()
            .map(|episodes| {
                let mut matches = episodes.iter().filter(|episode| episode.id == target.id);
                let first = matches.next().ok_or(Error::QueueChanged)?;
                if !compatible(&target, first)
                    || matches.any(|episode| !compatible(&target, episode))
                {
                    return Err(Error::QueueChanged);
                }
                Ok(first)
            })
            .transpose()?;
        let chart_has_track =
            clicked_chart_row.filter(|episode| identifier(&episode.raw["mainSong"]["id"]).is_ok());
        let room = self.together_detached_guard(token);
        let login = self.auth.pending_guard();
        let session = self.session.clone();
        let owner = token.clone();
        let bound = api.clone();
        let source_name = observation.lane.source();
        let guard = Cancellation::new(move || {
            login().is_ok()
                && !room.is_cancelled()
                && session.require_api(&owner, &bound).is_ok()
                && observation.current(&owner, &bound)
        });
        guard.check()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(token).await?;
        prepared.check(self, &before)?;
        let preparation = self.pending_preparation_guard(prepared, controller.clone());
        let preparing = {
            let guard = guard.clone();
            Cancellation::new(move || !guard.is_cancelled() && !preparation.is_cancelled())
        };
        let current = || preparing.check();
        current()?;
        if before.fm || queue.intelligence.is_some() {
            return Err(Error::State("podcast browse requires ordinary playback"));
        }
        let same_selected = before.selected.as_ref().is_some_and(|entry| {
            bound_episode(&entry.resource).is_some_and(|(episode, song, voice)| {
                episode == target.id
                    && target.song.as_ref().is_none_or(|id| &song == id)
                    && target
                        .radio
                        .as_ref()
                        .is_none_or(|id| voice["audio"]["id"] == *id)
            })
        });
        if same_selected
            && before.play_id.is_some()
            && matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            )
        {
            if double_click {
                let mode = if replace {
                    let raw = chart_has_track
                        .map(|episode| &episode.raw)
                        .or_else(|| {
                            before.selected.as_ref().and_then(|entry| {
                                bound_episode(&entry.resource).map(|(_, _, voice)| voice)
                            })
                        })
                        .ok_or(Error::QueueChanged)?;
                    Some(voice_replacement_mode(&raw["categoryId"], queue.mode))
                        .filter(|mode| *mode != queue.mode)
                } else {
                    None
                };
                return Ok((
                    PlaybackCommand::TogglePodcastRank {
                        expected: before.fence(),
                        paused: !before.paused,
                        mode,
                    },
                    guard,
                ));
            }
            return Ok((PlaybackCommand::Pause(!before.paused), guard));
        }

        let episode_id = target.id.as_str();
        let chart_category = chart_has_track.map(|episode| episode.raw["categoryId"].clone());
        let mut voices = if let Some(episodes) = chart {
            episodes
        } else {
            let around = self
                .session
                .run_for(token, |api, _| async move {
                    around_request(api.as_ref(), episode_id).await
                })
                .await;
            current()?;
            match around {
                Ok(raw) => {
                    let mut voices = parse_around_programs(&raw)?
                        .filter(|voices| !voices.is_empty())
                        .ok_or(Error::State("podcast browse around list is empty"))?;
                    let clicked = voices
                        .iter()
                        .find(|voice| voice.id == target.id)
                        .ok_or(Error::QueueChanged)?;
                    let radio = target
                        .radio
                        .as_deref()
                        .or_else(|| clicked.radio.as_ref().map(|v| v.id.as_str()));
                    if !compatible(&target, clicked) {
                        return Err(Error::QueueChanged);
                    }
                    let library = PodcastLibrary::new(self.session.clone())?;
                    let ascending = match radio {
                        Some(id) => library.radio_order_for(token, id)?.unwrap_or(false),
                        None => false,
                    };
                    order_around_programs(&mut voices, &raw, ascending);
                    voices
                }
                // M402 does not throw on a false/missing code. M71 does not
                // construct a list for that reply, so it is not a thrown fallback.
                Err(Error::Service { code, .. })
                    if code.is_null()
                        || code == 0
                        || code == "0"
                        || code == ""
                        || code == false =>
                {
                    return Err(Error::State("podcast browse around list is empty"));
                }
                Err(error) if request_failure(&error) => Vec::new(),
                Err(error) => return Err(error),
            }
        };
        let detail = self
            .session
            .run_for(token, |api, _| async move {
                PodcastApi::new(api).episode(&json!(episode_id)).await
            })
            .await?;
        current()?;
        if !compatible(&target, &detail)
            || detail.raw["grayDisplay"] == true
            || detail.song_id.is_none()
            || target
                .radio
                .as_ref()
                .is_some_and(|id| detail.radio.as_ref().is_none_or(|radio| &radio.id != id))
        {
            return Err(Error::QueueChanged);
        }
        if voices.is_empty() {
            voices.push(detail.clone());
        } else if let Some(clicked) = voices.iter_mut().find(|voice| voice.id == target.id) {
            if clicked
                .song_id
                .as_ref()
                .is_some_and(|song| detail.song_id.as_ref() != Some(song))
                || clicked.radio.as_ref().is_some_and(|radio| {
                    detail
                        .radio
                        .as_ref()
                        .is_none_or(|fresh| fresh.id != radio.id)
                })
            {
                return Err(Error::QueueChanged);
            }
            *clicked = detail.clone();
        }
        // Never copy the selected episode's media identity to a neighbour.
        // Missing neighbour metadata stays unplayable until a fresh read.
        let resources = voices
            .iter()
            .filter(|voice| voice.song_id.is_some())
            .map(resource)
            .collect::<Result<Vec<_>>>()?;
        let selected = resource(&detail)?;
        let source = json!({"kind":source_name,"scene":"unknown","pldScene":"audio"});
        let queue = if replace {
            QueueSnapshot {
                mode: voice_replacement_mode(
                    chart_category.as_ref().unwrap_or(&detail.raw["categoryId"]),
                    queue.mode,
                ),
                ..QueueSnapshot::default()
            }
        } else {
            queue
        };
        let random_batch = (double_click && queue.mode == PlaybackMode::Random).then(|| {
            let ids = resources
                .iter()
                .filter_map(|row| row["voice_id"].as_str().map(str::to_owned));
            (ids.clone().next(), ids.collect::<HashSet<_>>())
        });
        let mut queue = insert_collection_bound(queue, resources, source, false, true, true);
        let entry = queue
            .items
            .iter()
            .find(|entry| entry.resource == selected && entry.source["kind"] == source_name)
            .ok_or(Error::QueueChanged)?;
        queue.current = Some(entry.entry_id.clone());
        if let Some((Some(first_voice), ids)) = random_batch {
            let inserted: HashSet<_> = queue
                .items
                .iter()
                .filter(|entry| {
                    entry.resource["voice_id"]
                        .as_str()
                        .is_some_and(|id| ids.contains(id))
                        && entry.source["kind"] == source_name
                })
                .map(|entry| entry.entry_id.as_str())
                .collect();
            let first = queue
                .items
                .iter()
                .find(|entry| {
                    entry.resource["voice_id"] == first_voice
                        && inserted.contains(entry.entry_id.as_str())
                })
                .ok_or(Error::QueueChanged)?;
            queue.order = super::local_batch::random_order(
                &queue,
                &inserted,
                &first.entry_id,
                false,
                &mut rand::rng(),
            );
        }
        current()?;

        let position = self
            .podcast_browse_position(token, &api, &detail, &selected, &preparing)
            .await?;
        current()?;
        let first_source = controller
            .source_resolver()
            .prepare_scoped_episode(&selected, token, &api, &preparing, Some(&detail))
            .await?;
        current()?;
        // A preview is not permission to seek into full-length progress.
        let timeline = first_source.timeline(None)?;
        let position = position
            .filter(|position| timeline.to_media(*position).is_ok())
            .unwrap_or_else(|| timeline.snapshot().start);
        Ok((
            PlaybackCommand::PlayPodcastBrowse {
                update: QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: Some(QueueTransport {
                        position: Some(position),
                        paused: false,
                    }),
                },
                first_source,
            },
            guard,
        ))
    }

    pub(super) async fn podcast_browse_position(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        episode: &Episode,
        resource: &Value,
        guard: &Cancellation,
    ) -> Result<Option<f64>> {
        let Some(radio) = &episode.radio else {
            return Ok(None);
        };
        let treatment = api
            .verified_pc_experiment_revision()?
            .as_deref()
            .map(|revision| {
                self.pc_experiments.observed_group(
                    token,
                    api,
                    revision,
                    crate::podcast::resume::EXPERIMENT,
                )
            })
            .transpose()?
            .flatten()
            .as_deref()
            == Some("t1");
        if treatment {
            let enabled =
                crate::podcast::resume::read_for(&self.session, token, &json!(radio.id)).await;
            guard.check()?;
            match enabled {
                Ok(false) => return Ok(None),
                Ok(true) => {}
                Err(error) if request_failure(&error) || matches!(error, Error::Response(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let store = self.session.persistent_store()?;
        let latest = self.session.with_token(token, |owner| {
            store.read_snapshot(|tx| progress::latest::read(tx, owner, &radio.id))
        })?;
        if let Some(checkpoint) = latest
            && checkpoint.id == episode.id
            && episode.song_id.as_ref() == Some(&checkpoint.main_track_id)
        {
            return Ok(Some(
                if checkpoint.time < checkpoint.duration
                    && (checkpoint.time as f64) < episode.duration_ms as f64 / 1000.0
                {
                    checkpoint.time as f64
                } else {
                    0.0
                },
            ));
        }
        progress::record_position(&self.session, token, resource, false)
    }
}
