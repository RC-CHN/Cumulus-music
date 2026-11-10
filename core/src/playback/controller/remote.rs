use super::{
    PlaybackStatus, RemotePlayback, Response,
    load::{LoadOptions, LoadSource},
    state::Actor,
};
use crate::playback::source::ResolvedSource;
use crate::{Error, Result};
use std::sync::Arc;

impl Actor {
    pub(super) fn apply_together_external_queue(
        &mut self,
        value: super::TogetherExternalQueueUpdate,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<()> {
        if self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || value.queue.fm_source.is_some()
            || value.queue.intelligence.is_some()
            || reply.is_closed()
        {
            return Err(Error::QueueChanged);
        }
        if self.pending.is_some() {
            return Err(Error::MutationBusy);
        }
        let source = |row: &crate::playback::QueueEntry| {
            let mut source = row.source.clone();
            if let Some(value) = source.as_object_mut() {
                value.remove(crate::playback::intelligence::TRANSIENT);
            }
            source
        };
        match (self.queue.selected(), value.queue.selected()) {
            (None, None) => {}
            (Some(before), Some(after))
                if before.entry_id == after.entry_id
                    && before.resource == after.resource
                    && source(&before) == source(&after) => {}
            _ => return Err(Error::QueueChanged),
        }
        authority.check()?;
        self.session.with_token(&self.token, |_| {
            self.queue
                .install_together_external(value.queue, &value.room_id)
        })?;
        if let (Some(loaded), Some(item)) = (self.loaded.as_mut(), self.queue.selected()) {
            loaded.item = item;
        }
        Ok(())
    }

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
            return self.update_queue(value, None).await;
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

    pub(super) fn apply_offline_playlist_queue(
        &mut self,
        update: super::OfflinePlaylistQueueUpdate,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<Option<LoadOptions>> {
        if self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || update.queue.fm_source.is_some()
            || update.queue.intelligence.is_some()
            || self.queue.is_empty() != update.start
            || (update.start && (self.pending.is_some() || update.queue.selected().is_none()))
            || (!update.start && self.queue.selected() != update.queue.selected())
        {
            return Err(Error::QueueChanged);
        }
        self.session.with_token(&self.token, |owner| {
            self.queue
                .install_offline_playlist(update.queue, owner, &update.binding, authority)
        })?;
        // Keep the local index lease through the transaction. A selected
        // file is revalidated by the usual native decoder path afterwards.
        drop(update.index);
        // No reload, playback toggle or position change for an existing queue.
        Ok(update.start.then(LoadOptions::default))
    }

    pub(super) async fn apply_local_batch(
        &mut self,
        update: super::LocalBatchUpdate,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<Option<LoadOptions>> {
        if self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || self.pending.is_some()
            || self.quality_task.is_some()
            || update.queue.fm_source.is_some()
            || update.queue.intelligence.is_some()
        {
            return Err(Error::MutationBusy);
        }
        if !update.start {
            if self.queue.is_empty() || self.queue.selected() != update.queue.selected() {
                return Err(Error::QueueChanged);
            }
            authority.check()?;
            self.session
                .with_local_owner(&self.token, |_| self.queue.install(update.queue))?;
            return Ok(None);
        }
        let selected = update
            .queue
            .selected()
            .filter(|item| item.source["kind"] == "local" && item.resource["local_id"].is_string())
            .ok_or(Error::QueueChanged)?;
        // M308 does not toggle the current item from the toolbar. Rebind its
        // queue metadata only when both the native entry and file are exact.
        let retain = self.loaded.as_ref().is_some_and(|loaded| {
            self.state.active()
                && self.queue.selected().as_ref() == Some(&loaded.item)
                && loaded.item.entry_id == selected.entry_id
                && loaded.item.resource == selected.resource
        });
        self.save_position()?;
        authority.check()?;
        self.session
            .with_local_owner(&self.token, |_| self.queue.install(update.queue))?;
        // No scan/hide can replace the captured index before install. Release
        // before decoder start: the ordinary resolver rechecks the actual file.
        drop(update.index);
        if retain {
            self.loaded.as_mut().expect("retained local decoder").item = selected;
            if self.state == PlaybackStatus::Paused {
                self.pause(false).await?;
            }
            Ok(None)
        } else {
            Ok(Some(LoadOptions::default()))
        }
    }

    pub(super) async fn update_queue(
        &mut self,
        value: super::QueueUpdate,
        reply: Option<&Response>,
    ) -> Result<Option<LoadOptions>> {
        self.update_queue_with_source(value, reply, None).await
    }

    pub(super) async fn insert_next(
        &mut self,
        value: super::QueueUpdate,
        reply: Option<&Response>,
    ) -> Result<Option<LoadOptions>> {
        self.update_queue_inner(value, reply, None, true, None)
            .await
    }

    pub(super) async fn update_queue_with_source(
        &mut self,
        value: super::QueueUpdate,
        reply: Option<&Response>,
        first_source: Option<std::sync::Arc<crate::playback::source::ResolvedSource>>,
    ) -> Result<Option<LoadOptions>> {
        self.update_queue_inner(value, reply, first_source, false, None)
            .await
    }

    pub(super) async fn apply_search_group_next(
        &mut self,
        value: super::QueueUpdate,
        first_source: Option<std::sync::Arc<crate::playback::source::ResolvedSource>>,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<Option<LoadOptions>> {
        let selected = self.queue.selected();
        let next = value.queue.selected();
        let start = first_source.is_some();
        let valid = if let Some(first) = &first_source {
            self.queue.is_empty()
                && selected.is_none()
                && self.state == PlaybackStatus::Idle
                && self.play_id.is_none()
                && next.is_some()
                && value
                    .transport
                    .as_ref()
                    .is_some_and(|transport| !transport.paused && transport.position.is_none())
                && !first.needs_renewal_for_reopen(self.resolver.now())
        } else {
            selected.is_some()
                && selected == next
                && value.transport.is_none()
                && matches!(self.state, PlaybackStatus::Playing | PlaybackStatus::Paused)
                && self
                    .loaded
                    .as_ref()
                    .is_some_and(|loaded| Some(&loaded.item) == selected.as_ref())
        };
        if !valid
            || self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || value.queue.fm_source.is_some()
            || value.queue.intelligence.is_some()
        {
            return Err(Error::StaleResponse);
        }
        self.update_queue_inner(value, Some(reply), first_source, !start, Some(authority))
            .await
    }

    pub(super) async fn apply_search_group_play(
        &mut self,
        mut value: super::QueueUpdate,
        first_source: Option<Arc<ResolvedSource>>,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<Option<LoadOptions>> {
        let selected = self.queue.selected();
        let next = value.queue.selected().ok_or(Error::QueueChanged)?;
        let empty = self.queue.is_empty()
            && selected.is_none()
            && self.state == PlaybackStatus::Idle
            && self.play_id.is_none();
        let active = matches!(self.state, PlaybackStatus::Playing | PlaybackStatus::Paused)
            && self.play_id.is_some()
            && self
                .loaded
                .as_ref()
                .is_some_and(|loaded| Some(&loaded.item) == selected.as_ref());
        let retain = active && selected.as_ref() == Some(&next);
        let fresh = first_source.as_ref().is_some_and(|source| {
            !source.needs_renewal_for_reopen(self.resolver.now())
                && next.source["scene"] == "search"
                && next.resource["trial_mode"] == 42
        });
        let source_is_bound = if retain {
            first_source.is_none()
        } else {
            fresh
        };
        if (!empty && !active)
            || !source_is_bound
            || self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || value.queue.fm_source.is_some()
            || value.queue.intelligence.is_some()
            || value
                .transport
                .as_ref()
                .is_none_or(|transport| transport.paused || transport.position.is_some())
        {
            return Err(Error::StaleResponse);
        }
        // Play of the current song keeps the same loaded entry and clock.
        // Let the actor resume explicitly instead of applying a remote seek.
        if retain {
            value.transport = None;
        }
        let options = self
            .update_queue_inner(value, Some(reply), first_source, false, Some(authority))
            .await?;
        if retain && self.state == PlaybackStatus::Paused {
            authority.check()?;
            self.pause(false).await?;
        }
        Ok(options)
    }

    async fn update_queue_inner(
        &mut self,
        value: super::QueueUpdate,
        reply: Option<&Response>,
        first_source: Option<std::sync::Arc<crate::playback::source::ResolvedSource>>,
        insert_next: bool,
        authority: Option<&crate::playback::engine::Cancellation>,
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
        let previous_revision = self.queue.revision();
        self.save_position()?;
        if let Some(authority) = authority {
            if reply.is_some_and(Response::is_closed) {
                return Err(Error::StaleResponse);
            }
            authority.check()?;
        }
        if insert_next || first_source.is_some() || authority.is_some() {
            self.session
                .with_token(&self.token, |_| self.queue.install(value.queue))?;
        } else {
            self.queue.install(value.queue)?;
        }
        if insert_next && self.queue.revision() != previous_revision {
            self.ordinary_chorus_trigger(super::ChorusTrigger::InsertNext(self.queue.revision()));
        }
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
        if first_source.is_some()
            || before != after
            || self.loaded.is_none()
            || !self.state.active()
        {
            return Ok(Some(LoadOptions {
                paused,
                position: if before.as_ref().map(|e| &e.entry_id)
                    == after.as_ref().map(|e| &e.entry_id)
                {
                    position.or(self.position)
                } else {
                    position
                },
                // Let the authorized source clamp a stale checkpoint, but do
                // not fall through to a fresh authorization at this selection.
                recover: true,
                source: first_source.map_or_else(
                    super::load::LoadSource::default,
                    super::load::LoadSource::Retained,
                ),
                ..Default::default()
            }));
        }
        if let Some(transport) = value.transport {
            self.apply_remote(
                RemotePlayback {
                    expected: value.expected,
                    entry_id: after.expect("checked").entry_id,
                    position: transport.position.or(self.position).unwrap_or(0.0),
                    paused,
                },
                reply,
            )
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
        reply: Option<&Response>,
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
        let loaded = self.loaded.as_ref().expect("checked");
        let position = loaded.timeline.to_media(value.position)?;
        let entry = loaded.media.entry_id;
        let local_equalizer = loaded.local_equalizer;
        let item = loaded.item.clone();
        let cancel = self.guard();
        let seek_cancel = cancel.clone();
        let result = self
            .parametric_transport(value.paused, cancel.clone(), move |engine| {
                if value.paused {
                    engine.pause(true)?;
                }
                let replacement = engine.seek_with_receipt(position, entry, &seek_cancel)?;
                if !local_equalizer {
                    engine.pause(value.paused)?;
                }
                Ok(replacement)
            })
            .await;
        match result {
            Ok(Some(media)) => {
                let resume = async {
                    if reply.is_some_and(Response::is_closed) {
                        return Err(Error::Cancelled);
                    }
                    self.queue.reload()?;
                    if self.queue.selected().as_ref() != Some(&item) {
                        return Err(Error::QueueChanged);
                    }
                    let entry = media.entry_id;
                    self.parametric_transport(value.paused, cancel, move |engine| {
                        if engine.local_equalizer_entry() != Some(entry) {
                            return Err(Error::StaleResponse);
                        }
                        engine.pause(value.paused)
                    })
                    .await
                }
                .await;
                if let Err(error) = resume {
                    self.stop_after_poll_failure(&error).await;
                    return Err(error);
                }
                self.loaded.as_mut().expect("checked").media = media;
                self.native_seeking = true;
                self.deferred_events.clear();
            }
            Ok(None) => {}
            Err(error) => {
                // Never present a partly applied command as success. Stop after an
                // engine failure; the together inbox retains recovery responsibility.
                let _ = self.stop(false).await;
                self.state = PlaybackStatus::Error;
                self.record_failure(&error);
                return Err(error);
            }
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
