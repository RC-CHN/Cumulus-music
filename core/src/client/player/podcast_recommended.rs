//! Ordinary recommendation AudioCard overlay (M3108 -> RoundPlayButton -> M404).
use super::{MusicClient, PlaybackCommand, PlaybackStatus, PlayerOrigin, origin};
use crate::{
    Error, Result,
    account::SessionToken,
    client::podcast::radio_playback::by_radio,
    playback::engine::Cancellation,
    podcast::{Episode, PodcastApi, PodcastLibrary, PodcastScope, progress::latest},
    protocol::api::identifier,
};
use serde_json::{Value, json};

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

impl MusicClient {
    pub(super) async fn recommended_podcast_command(
        &self,
        token: &SessionToken,
        receipt: &str,
        revision: &str,
        id: &str,
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
        let current = self.player.controller().await?.snapshot();
        if current.account == *token && !current.fm && current.play_id.is_some()
            && matches!(current.state, PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering)
            && current.selected.as_ref().is_some_and(|entry| {
                identity_voice(&entry.resource)
                    && matches!(origin::project(&entry.source), Some(PlayerOrigin::Podcast { id: current }) if current == id)
            })
        {
            return Ok((PlaybackCommand::Pause(!current.paused), guard));
        }
        let ascending = library.radio_order_for(token, &id)?;
        let mut episodes = self
            .session
            .run_for(token, |api, _| {
                let (id, guard) = (&id, &guard);
                async move {
                    // The M404 around-program branch requires an assigned
                    // ID equal to the latest voice; this overlay has none.
                    by_radio(api.as_ref(), id, ascending, guard).await
                }
            })
            .await?;
        guard.check()?;
        if episodes.is_empty() {
            return Err(Error::State("recommended podcast is empty"));
        }

        let cohort = api
            .verified_pc_experiment_revision()?
            .as_deref()
            .map(|revision| {
                self.pc_experiments
                    .observed_group(token, &api, revision, RESUME_EXPERIMENT)
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
            read_resume(id.clone()).await?
        } else {
            true
        };
        let latest = if allow_resume {
            let store = self.session.persistent_store()?;
            self.session.with_token(token, |owner| {
                store.read_snapshot(|tx| latest::read(tx, owner, &id))
            })?
        } else {
            None
        };
        let selected = match &latest {
            Some(checkpoint) => episodes
                .iter()
                .position(|voice| voice.id == checkpoint.id)
                .ok_or(Error::QueueChanged)?,
            None => 0,
        };
        let mut resources = Vec::with_capacity(episodes.len());
        let mut start = None;
        for (index, mut voice) in episodes.drain(..).enumerate() {
            guard.check()?;
            if voice.song_id.is_none() {
                if index != selected {
                    continue;
                }
                let program_id = json!(voice.id);
                voice = self
                    .session
                    .run_for(token, |api, _| async move {
                        PodcastApi::new(api).episode(&program_id).await
                    })
                    .await?;
            }
            if voice.radio.is_none() {
                voice.radio = Some(radio.clone());
            }
            let mut resource = voice.playback_resource()?;
            resource["song"] = voice.download_metadata()?;
            resource["level"] = json!("standard");
            if index == selected {
                let position = if let Some(checkpoint) = &latest {
                    if voice.radio.as_ref().is_none_or(|source| source.id != id)
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
                start = Some((resources.len(), position));
            }
            resources.push(resource);
        }
        let (index, position) = start.ok_or(Error::QueueChanged)?;
        guard.check()?;
        check_list()?;
        // M7 category IDs for anime, electric and cover retain prior mode.
        let sequential = !matches!(radio.raw["categoryId"].as_i64(), Some(3001 | 10002 | 2001));
        Ok((
            PlaybackCommand::ReplacePodcastRecommendation {
                resources,
                index,
                position,
                sequential,
                source: json!({"kind":"podcast_subscription_recommendation","id":id,"scene":"unknown"}),
            },
            guard,
        ))
    }
}

fn identity_voice(resource: &Value) -> bool {
    identifier(&resource["voice_id"]).is_ok()
}
