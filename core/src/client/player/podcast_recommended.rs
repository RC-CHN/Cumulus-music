//! Ordinary recommendation AudioCard overlay (M3108 -> RoundPlayButton -> M404).
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, PlayerOrigin, PodcastRecommendationAction,
    origin, queue_playlist::insert_collection_bound,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::podcast::radio_playback::by_radio,
    playback::{controller::QueueUpdate, engine::Cancellation},
    podcast::{Episode, PodcastApi, PodcastLibrary, PodcastScope, Radio, progress::latest},
    protocol::api::Api,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::sync::Arc;

const RESUME_EXPERIMENT: &str = "Podcast_ResumePlaySwitch";

fn episode_progress(episode: &Episode) -> f64 {
    let record = &episode.raw["djPlayRecordVo"];
    let location = record["listenLocation"].as_f64().unwrap_or(0.0);
    if record["isListened"] == true || !(0.0..(episode.duration_ms as f64)).contains(&location) {
        0.0
    } else {
        location / 1000.0
    }
}

pub(super) struct RadioPlaybackPlan {
    pub(super) resources: Vec<Value>,
    pub(super) start: Option<RadioPlaybackTarget>,
}
pub(super) struct RadioPlaybackTarget {
    pub(super) index: usize,
    pub(super) position: f64,
    // Only a fresh detail fetched here may satisfy the later source gate.
    pub(super) detail: Option<Episode>,
}

pub(super) enum RadioPlaybackStart {
    Overlay { select_checkpoint: bool },
    Table { mode: crate::playback::PlaybackMode },
}

pub(super) fn same_radio_transport(
    current: &crate::playback::controller::PlaybackSnapshot,
    token: &SessionToken,
    id: &str,
) -> Option<PlaybackCommand> {
    (current.account == *token
        && !current.fm
        && current.play_id.is_some()
        && matches!(
            current.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
        && current.selected.as_ref().is_some_and(|entry| {
            identity_voice(&entry.resource)
                && matches!(origin::project(&entry.source), Some(PlayerOrigin::Podcast { id: current }) if current == id)
        }))
    .then(|| PlaybackCommand::Pause(!current.paused))
}

impl MusicClient {
    pub(super) async fn recommended_podcast_command(
        &self,
        token: &SessionToken,
        receipt: &str,
        revision: &str,
        id: &str,
        action: PodcastRecommendationAction,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        if receipt.len() != 32 || id.len() > 32 {
            return Err(Error::InvalidInput("invalid recommended podcast selection"));
        }
        let id = identifier(&json!(id))?;
        let (radio, receipt_guard) = self.subscription_recommendation(token, receipt, &id)?;
        let room = self.together_detached_guard(token);
        if room.is_cancelled() {
            return Err(Error::State(
                "recommended podcast requires ordinary playback",
            ));
        }
        let library = PodcastLibrary::new(self.session.clone())?;
        let check_list = || -> Result<()> {
            if library
                .cached_for(token, &PodcastScope::Podcasts)?
                .is_none_or(|list| list.revision != revision || list.remote_total >= 50)
            {
                return Err(Error::QueueChanged);
            }
            Ok(())
        };
        check_list()?;
        let api = self.session.native_api(token)?;
        let guard = {
            let session = self.session.clone();
            let expected = token.clone();
            let api = api.clone();
            Cancellation::new(move || {
                !receipt_guard.is_cancelled()
                    && !room.is_cancelled()
                    && session.require_api(&expected, &api).is_ok()
            })
        };
        let controller = self.player.controller().await?;
        let next_queue = if action == PodcastRecommendationAction::Next {
            let (current, queue) = controller.queue_with_state(token).await?;
            if current.fm || queue.intelligence.is_some() {
                return Err(Error::State(
                    "recommended podcast requires ordinary playback",
                ));
            }
            if !queue.items.is_empty()
                && (queue.current.is_none()
                    || matches!(
                        current.state,
                        PlaybackStatus::Preparing | PlaybackStatus::Loading
                    ))
            {
                return Err(Error::State("recommended podcast queue is not settled"));
            }
            (!queue.items.is_empty()).then_some((current, queue))
        } else {
            None
        };
        if action == PodcastRecommendationAction::Overlay
            && let Some(command) = same_radio_transport(&controller.snapshot(), token, &id)
        {
            return Ok((command, guard));
        }
        let RadioPlaybackPlan { resources, start } = self
            .prepare_radio_playback(
                token,
                &api,
                &radio,
                &guard,
                RadioPlaybackStart::Overlay {
                    select_checkpoint: next_queue.is_none(),
                },
            )
            .await?;
        let RadioPlaybackTarget {
            index, position, ..
        } = start.ok_or(Error::QueueChanged)?;
        let source =
            json!({"kind":"podcast_subscription_recommendation","id":id,"scene":"unknown"});
        check_list()?;
        if let Some((before, queue)) = next_queue {
            // An existing queue is edited without claiming audio focus or
            // loading a decoder. Preserve even a paused selected entry.
            let queue = insert_collection_bound(queue, resources, source, false, true, false);
            return Ok((
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: None,
                }),
                guard,
            ));
        }
        // M7 category IDs for anime, electric and cover retain prior mode.
        // M3085 only switches mode when the original action clears the queue.
        let sequential = action != PodcastRecommendationAction::Next
            && !matches!(radio.raw["categoryId"].as_i64(), Some(3001 | 10002 | 2001));
        Ok((
            PlaybackCommand::ReplacePodcastCollection {
                resources,
                index,
                position,
                sequential,
                first_source: None,
                source,
            },
            guard,
        ))
    }

    /// Shared M404/M3085 radio playback plan. The caller owns its own UI/result
    /// authority and chooses whether this action can select a stored checkpoint.
    pub(super) async fn prepare_radio_playback(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        radio: &Radio,
        guard: &Cancellation,
        policy: RadioPlaybackStart,
    ) -> Result<RadioPlaybackPlan> {
        let id = radio.id.as_str();
        let library = PodcastLibrary::new(self.session.clone())?;
        let ascending = library.radio_order_for(token, id)?;
        let mut episodes = self
            .session
            .run_for(token, |api, _| async move {
                // The M404 around-program branch requires an assigned
                // ID equal to the latest voice; these overlays have none.
                by_radio(api.as_ref(), id, ascending, guard).await
            })
            .await?;
        guard.check()?;
        if episodes.is_empty() {
            return Err(Error::State("podcast playback list is empty"));
        }

        let cohort = api
            .verified_pc_experiment_revision()?
            .as_deref()
            .map(|revision| {
                self.pc_experiments
                    .observed_group(token, api, revision, RESUME_EXPERIMENT)
            })
            .transpose()?
            .flatten();
        let read_resume = |radio_id: String| async move {
            let setting = self
                .session
                .run_for(token, |api, _| async move {
                    PodcastApi::new(api).resume_enabled(&json!(radio_id)).await
                })
                .await;
            match setting {
                // The original setting reader returns null on failure; an
                // actual identity or authentication change is never a default.
                Err(Error::Service { code, .. })
                    if code != 301 && code != 401 && code != "301" && code != "401" =>
                {
                    Ok(true)
                }
                Err(
                    Error::Transport(_)
                    | Error::Response(_)
                    | Error::HttpStatus {
                        status: 500..=599, ..
                    },
                ) => Ok(true),
                other => other,
            }
        };
        let treatment = cohort.as_deref() == Some("t1");
        let allow_resume = if treatment {
            read_resume(id.to_owned()).await?
        } else {
            true
        };
        let latest = if allow_resume
            && !matches!(
                policy,
                RadioPlaybackStart::Overlay {
                    select_checkpoint: false
                }
            ) {
            let store = self.session.persistent_store()?;
            self.session.with_token(token, |owner| {
                store.read_snapshot(|tx| latest::read(tx, owner, id))
            })?
        } else {
            None
        };
        let table = matches!(policy, RadioPlaybackStart::Table { .. });
        if table {
            episodes.truncate(super::queue_playlist::QUEUE_LIMIT);
        }
        let selected = match (&latest, &policy) {
            (Some(checkpoint), _) => {
                let selected = episodes.iter().position(|voice| voice.id == checkpoint.id);
                if selected.is_none() && matches!(policy, RadioPlaybackStart::Overlay { .. }) {
                    return Err(Error::QueueChanged);
                }
                selected
            }
            (
                None,
                RadioPlaybackStart::Table {
                    mode: crate::playback::PlaybackMode::Random,
                },
            ) => {
                use rand::seq::IteratorRandom;
                episodes
                    .iter()
                    .enumerate()
                    .filter(|(_, voice)| voice.raw["grayDisplay"] != true)
                    .map(|(index, _)| index)
                    .choose(&mut rand::rng())
            }
            (None, RadioPlaybackStart::Table { .. }) => episodes
                .iter()
                .position(|voice| voice.raw["grayDisplay"] != true),
            (None, _) => Some(0),
        };
        let mut resources = Vec::with_capacity(episodes.len());
        let mut start = None;
        for (index, mut voice) in episodes.drain(..).enumerate() {
            guard.check()?;
            if table && voice.raw["grayDisplay"] == true {
                if Some(index) == selected {
                    return Err(Error::State("selected podcast episode is unavailable"));
                }
                continue;
            }
            if voice.song_id.is_none() && Some(index) != selected {
                continue;
            }
            let mut selected_detail = None;
            if Some(index) == selected && (table || voice.song_id.is_none()) {
                let program_id = json!(voice.id);
                let detail = self
                    .session
                    .run_for(token, |api, _| async move {
                        PodcastApi::new(api).episode(&program_id).await
                    })
                    .await?;
                guard.check()?;
                if table
                    && (detail.id != voice.id
                        || detail.raw["grayDisplay"] == true
                        || voice
                            .song_id
                            .as_ref()
                            .is_some_and(|id| detail.song_id.as_ref() != Some(id))
                        || voice.radio.as_ref().is_some_and(|observed| {
                            detail
                                .radio
                                .as_ref()
                                .is_some_and(|fresh| fresh.id != observed.id)
                        }))
                {
                    return Err(Error::QueueChanged);
                }
                if voice.song_id.is_none() {
                    voice = detail.clone();
                }
                if table {
                    // M15.Et preserves the episode's own parent even in a
                    // curated radio. The listing authority is the container.
                    if voice.radio.is_none() {
                        voice.radio = detail.radio.clone();
                    }
                    selected_detail = Some(detail);
                }
            }
            if voice.radio.is_none() {
                voice.radio = Some(radio.clone());
            }
            let mut resource = voice.playback_resource()?;
            resource["song"] = voice.download_metadata()?;
            resource["level"] = json!("standard");
            if Some(index) == selected {
                let position = if let Some(checkpoint) = &latest {
                    if (!table && voice.radio.as_ref().is_none_or(|source| source.id != id))
                        || voice.song_id.as_deref() != Some(checkpoint.main_track_id.as_str())
                    {
                        return Err(Error::QueueChanged);
                    }
                    if checkpoint.time < checkpoint.duration {
                        checkpoint.time as f64
                    } else {
                        0.0
                    }
                } else {
                    let episode_resume = if treatment {
                        match &voice.radio {
                            Some(episode_radio) if episode_radio.id != id => {
                                read_resume(episode_radio.id.clone()).await?
                            }
                            _ => allow_resume,
                        }
                    } else {
                        allow_resume
                    };
                    if episode_resume {
                        episode_progress(&voice)
                    } else {
                        0.0
                    }
                };
                start = Some(RadioPlaybackTarget {
                    index: resources.len(),
                    position,
                    detail: selected_detail,
                });
            }
            resources.push(resource);
        }
        if resources.is_empty() || (selected.is_some() && start.is_none()) {
            return Err(Error::QueueChanged);
        }
        guard.check()?;
        Ok(RadioPlaybackPlan { resources, start })
    }
}

fn identity_voice(resource: &Value) -> bool {
    identifier(&resource["voice_id"]).is_ok()
}
