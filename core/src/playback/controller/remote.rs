use super::{
    PlaybackStatus, RemotePlayback,
    load::{LoadOptions, LoadSource},
    state::Actor,
};
use crate::playback::source::ResolvedSource;
use crate::{Error, Result};
use std::sync::Arc;

impl Actor {
    pub(super) async fn update_zone_fm(
        &mut self,
        value: super::QueueUpdate,
        retain_paused: bool,
    ) -> Result<Option<LoadOptions>> {
        use crate::{playback::engine::MediaInput, protocol::api::identifier};
        let previous = self.queue.selected();
        let following = value
            .queue
            .items
            .iter()
            .find(|entry| Some(entry.entry_id.as_str()) == value.queue.current.as_deref());
        let compatible = previous.as_ref().zip(following).is_some_and(|(old, new)| {
            let zone = |entry: &crate::playback::QueueEntry| {
                entry.source["kind"] == "fm"
                    && entry.source["source"]["kind"] == "zone"
                    && entry.resource["local_id"].is_null()
                    && entry.resource["download_task_id"].is_null()
                    && entry.resource["cloud_song_id"].is_null()
                    && entry.resource["voice_id"].is_null()
                    && entry.resource["playback_constraint"].is_null()
            };
            zone(old)
                && zone(new)
                && old.source["source"]["rule_id"] != new.source["source"]["rule_id"]
                && identifier(&old.resource["id"])
                    .is_ok_and(|id| identifier(&new.resource["id"]).is_ok_and(|other| id == other))
                && old.resource["trial_mode"].as_i64().is_some()
                && old.resource["trial_mode"] == new.resource["trial_mode"]
                && old.resource["level"] == new.resource["level"]
                && self.loaded.as_ref().is_some_and(|loaded| {
                    loaded.item == *old && matches!(loaded.source.input, MediaInput::Url(_))
                })
        });
        if !compatible
            || self.pending.is_some()
            || self.quality_task.is_some()
            || !self.state.active()
        {
            return self.update_queue(value).await;
        }
        value.queue.validate()?;
        self.save_position()?;
        self.queue.install(value.queue)?;
        // The selected source, lease and timeline are still the exact
        // authorized online track. Rebind only its rule/entry metadata.
        let selected = self.queue.selected().ok_or(Error::QueueChanged)?;
        self.loaded.as_mut().expect("compatible loaded source").item = selected;
        if (self.state == PlaybackStatus::Paused) != retain_paused {
            self.pause(retain_paused).await?;
        }
        Ok(None)
    }

    pub(super) async fn replace_follow_latest(
        &mut self,
        resources: Vec<serde_json::Value>,
        first_download: Option<Arc<ResolvedSource>>,
    ) -> Result<Option<LoadOptions>> {
        self.replace_collection_preserving_song(
            resources,
            serde_json::json!({"kind":"follow_latest","scene":"album","href":"/newSongPublish",
                "resourceType":"track","originalResourceType":"album"}),
            first_download,
        )
        .await
    }

    pub(super) async fn replace_collection_preserving_song(
        &mut self,
        mut resources: Vec<serde_json::Value>,
        source: serde_json::Value,
        first_download: Option<Arc<ResolvedSource>>,
    ) -> Result<Option<LoadOptions>> {
        if self.queue.is_fm() || self.quality_task.is_some() {
            return Err(Error::MutationBusy);
        }
        let first = resources
            .first()
            .filter(|item| item["id"].is_string())
            .ok_or(Error::InvalidInput("missing first collection song"))?;
        let first_id = first["id"].as_str().expect("checked first ID");
        // M/308 replaces the list, then retains or resumes the decoder when
        // that same authorized source was already current. A downloaded
        // exception must never retain an online decoder just by numeric ID.
        let retained = self
            .loaded
            .as_ref()
            .filter(|loaded| {
                use crate::playback::engine::MediaInput;
                let same_source = if let Some(source) = &first_download {
                    self.independent_transport(&loaded.item)
                        && loaded.item.resource["cloud_song_id"].is_null()
                        && loaded.item.resource["voice_id"].is_null()
                        && loaded.item.resource["download_task_id"] == first["download_task_id"]
                        && loaded.item.resource["path"] == first["path"]
                        && matches!((&loaded.source.input, &source.input),
                            (MediaInput::File(a), MediaInput::File(b)) if a == b)
                } else {
                    self.ordinary_online_song(&loaded.item)
                };
                self.pending.is_none()
                    && self.state.active()
                    && self.queue.selected().as_ref() == Some(&loaded.item)
                    && same_source
                    && crate::protocol::api::identifier(&loaded.item.resource["id"])
                        .is_ok_and(|id| id == first_id)
            })
            .map(|loaded| (loaded.item.entry_id.clone(), loaded.item.resource.clone()));

        self.stamp_quality(&mut resources);
        if let Some((_, resource)) = &retained {
            // Keep the loaded source's native settings, including a quality
            // selected for this active entry; only update public catalog data.
            let song = resources[0]["song"].take();
            let duration = resources[0]["dt"].take();
            resources[0] = resource.clone();
            resources[0]["song"] = song;
            resources[0]["dt"] = duration;
        }
        self.queue.replace_with_selected_identity(
            resources,
            0,
            source,
            retained.as_ref().map(|(entry_id, _)| entry_id.clone()),
        )?;
        if retained.is_none() {
            return Ok(Some(LoadOptions {
                source: first_download.map(LoadSource::Retained).unwrap_or_default(),
                ..Default::default()
            }));
        }
        // Keep actor identity aligned with the persisted replacement while
        // keeping the actual media, timeline, history and playback ID.
        let selected = self
            .queue
            .selected()
            .expect("replacement selected its first song");
        self.loaded.as_mut().expect("retained current").item = selected;
        if self.state == PlaybackStatus::Paused {
            self.pause(false).await?;
        }
        Ok(None)
    }

    pub(super) async fn update_queue(
        &mut self,
        value: super::QueueUpdate,
    ) -> Result<Option<LoadOptions>> {
        value.queue.validate()?;
        if value
            .transport
            .as_ref()
            .is_some_and(|t| t.position.is_some_and(|p| !p.is_finite() || p < 0.0))
        {
            return Err(Error::InvalidInput("invalid shared queue position"));
        }
        let before = self.queue.selected();
        let paused = value.transport.as_ref().map_or(
            !matches!(
                self.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering
            ),
            |t| t.paused,
        );
        let position = value.transport.as_ref().and_then(|t| t.position);
        self.save_position()?;
        self.queue.install(value.queue)?;
        let after = self.queue.selected();
        if after.is_none() {
            self.stop(false).await?;
            return Ok(None);
        }
        if value.transport.is_none()
            && matches!(
                self.state,
                PlaybackStatus::Idle | PlaybackStatus::Ended | PlaybackStatus::Error
            )
        {
            // A metadata/constraint cleanup is not a new playback request.
            // Preserve an explicit local stop or failure until the user plays.
            if before != after {
                // The old selection's EOF must not tell the FM watcher to
                // naturally advance the newly selected, still stopped item.
                let state = if self.state == PlaybackStatus::Ended {
                    PlaybackStatus::Idle
                } else {
                    self.state
                };
                self.stop(false).await?;
                self.state = state;
            }
            return Ok(None);
        }
        if before != after || self.loaded.is_none() || !self.state.active() {
            return Ok(Some(LoadOptions {
                paused,
                position: if before.as_ref().map(|e| &e.entry_id)
                    == after.as_ref().map(|e| &e.entry_id)
                {
                    position.or(self.position)
                } else {
                    position
                },
                recover: true,
                ..Default::default()
            }));
        }
        if let Some(transport) = value.transport {
            self.apply_remote(RemotePlayback {
                expected: value.expected,
                entry_id: after.expect("checked").entry_id,
                position: transport.position.or(self.position).unwrap_or(0.0),
                paused,
            })
            .await
        } else {
            Ok(None)
        }
    }
    pub(super) async fn restore_fm(
        &mut self,
        input: super::FmRestore,
    ) -> Result<Option<LoadOptions>> {
        match self.queue.install_fm_restore(&input)? {
            crate::playback::FmRestoreOutcome::AlreadyOrdinary => Ok(None),
            crate::playback::FmRestoreOutcome::Restored(position) => {
                if self.queue.selected().is_none() {
                    self.stop(false).await?;
                    return Ok(None);
                }
                Ok(Some(LoadOptions {
                    paused: true,
                    position,
                    recover: true,
                    ..Default::default()
                }))
            }
        }
    }

    pub(super) async fn apply_remote(
        &mut self,
        value: RemotePlayback,
    ) -> Result<Option<LoadOptions>> {
        if !value.position.is_finite() || value.position < 0.0 {
            return Err(Error::InvalidInput("invalid remote playback position"));
        }
        // Fence checking and intent revision are handled by the actor before
        // this method. Selection, load options and same-track controls are one
        // request; no intermediate Select may accidentally start paused audio.
        let selected = self.queue.selected();
        if selected
            .as_ref()
            .is_none_or(|e| e.entry_id != value.entry_id)
            || !self.state.active()
            || self.loaded.is_none()
        {
            self.save_position()?;
            self.queue.select(&value.entry_id)?;
            return Ok(Some(LoadOptions {
                position: Some(value.position),
                paused: value.paused,
                ..Default::default()
            }));
        }
        let position = self
            .loaded
            .as_ref()
            .expect("checked")
            .timeline
            .to_media(value.position)?;
        let result = self
            .parametric_transport(value.paused, self.guard(), move |engine| {
                if value.paused {
                    engine.pause(true)?;
                }
                engine.seek(position)?;
                engine.pause(value.paused)
            })
            .await;
        if let Err(error) = result {
            // Never present a partly applied command as success. Stop after an
            // engine failure; the together inbox retains recovery responsibility.
            let _ = self.stop(false).await;
            self.state = PlaybackStatus::Error;
            self.record_failure(&error);
            return Err(error);
        }
        self.position = Some(value.position);
        self.paused_seek = value.paused;
        self.buffered_through = None;
        self.seek_revision = self.control_revision;
        self.state = if value.paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        };
        self.persist_transport().await?;
        Ok(None)
    }
}
