use super::{
    HistoryRowQueueAction, PlaybackStatus, RemotePlayback, Response,
    load::{LoadOptions, LoadSource},
    state::Actor,
};
use crate::playback::source::ResolvedSource;
use crate::podcast::LikedPageQueueFence;
use crate::{Error, Result};
use std::sync::Arc;

#[cfg(test)]
mod liked_queue_tests;

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

    pub(super) async fn apply_history_batch(
        &mut self,
        mut update: super::HistoryQueueUpdate,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
    ) -> Result<Option<LoadOptions>> {
        if (!update.play_all && (self.queue.is_fm() || self.queue.intelligence_context().is_some()))
            || self.pending.is_some()
            || self.quality_task.is_some()
            || update.queue.fm_source.is_some()
            || update.queue.intelligence.is_some()
        {
            return Err(Error::MutationBusy);
        }
        if let Some(action) = update.row_menu
            && (update.play_all
                || update.start != (action == HistoryRowQueueAction::Play || update.was_empty)
                || !matches!(
                    update.kind,
                    crate::history::HistoryKind::Song | crate::history::HistoryKind::Voice
                ))
        {
            return Err(Error::InvalidInput("invalid history row menu update"));
        }
        if self.queue.is_empty() != update.was_empty
            || (!update.start && (update.was_empty || update.play_all))
            || (update.start && update.queue.selected().is_none())
            || (!update.start && self.queue.selected() != update.queue.selected())
        {
            return Err(Error::QueueChanged);
        }
        if reply.is_closed() {
            return Err(Error::StaleResponse);
        }
        authority.check()?;
        let row_play = update.row_menu == Some(HistoryRowQueueAction::Play);
        let retained = if update.start && !update.play_all && self.state.active() {
            self.loaded.as_ref().and_then(|loaded| {
                let mut selected = update.queue.selected()?;
                if self.queue.selected().as_ref() != Some(&loaded.item)
                    || selected.entry_id != loaded.item.entry_id
                    || row_play
                        && (!self.independent_transport(&loaded.item)
                            || !self.independent_transport(&selected))
                {
                    return None;
                }
                // Replace stamps the current preference for ordinary songs.
                // That stamp alone does not change the actual loaded media.
                let unstamp = |mut value: serde_json::Value| {
                    if let Some(object) = value.as_object_mut() {
                        object.remove("quality_preference_revision");
                    }
                    value
                };
                let same_resource =
                    unstamp(selected.resource.clone()) == unstamp(loaded.item.resource.clone());
                if !same_resource
                    && (!row_play || !self.same_history_row_online_binding(loaded, &selected))
                {
                    return None;
                }
                let fresh = std::mem::replace(&mut selected.resource, loaded.item.resource.clone());
                if !same_resource {
                    // M308 updates current presentation on the same decoder.
                    // Keep its exact authorized source and quality selection.
                    for field in ["song", "dt"] {
                        if let Some(value) = fresh.get(field) {
                            selected.resource[field] = value.clone();
                        }
                    }
                }
                Some(selected)
            })
        } else {
            None
        };
        if update.start && !update.was_empty {
            self.save_position()?;
            authority.check()?;
        }
        if let Some(retained) = &retained {
            let selected = update
                .queue
                .items
                .iter_mut()
                .find(|item| item.entry_id == retained.entry_id)
                .ok_or(Error::QueueChanged)?;
            *selected = retained.clone();
        }
        let session = self.session.clone();
        let store = session.persistent_store()?;
        let publish = |owner: &str| {
            if let Some(binding) = &update.shuffle_anchor {
                if !update.start
                    || update.kind != crate::history::HistoryKind::Voice
                    || (!update.play_all
                        && update.row_menu.is_none()
                        && update.queue.mode != crate::playback::PlaybackMode::Random)
                    || !update
                        .queue
                        .selected()
                        .is_some_and(|item| binding.anchor.matches(&item.resource))
                {
                    return Err(Error::QueueChanged);
                }
                binding.setting.check()?;
                binding.anchor.require_current(&store, owner)?;
            }
            if update.play_all {
                self.queue.install_history_play_all(
                    update.queue,
                    owner,
                    update.kind,
                    &update.revision,
                    update.fm_backup_queue.as_ref(),
                )
            } else {
                self.queue.install(update.queue)
            }
        };
        if row_play {
            // Cancellation before the native commit is a refusal. Once the
            // queue is installed, the actor's account/generation guard owns
            // transport resume; a retired caller must not report a refusal
            // after the queue has already changed.
            if reply.is_closed() {
                return Err(Error::StaleResponse);
            }
            authority.check()?;
        }
        if update.play_all {
            session.with_local_owner(&self.token, publish)?;
        } else {
            crate::history::HistoryService::new(session)?.with_matching_revision(
                &self.token,
                update.kind,
                &update.revision,
                publish,
            )?;
        }
        if let Some(retained) = retained {
            self.loaded.as_mut().expect("retained history decoder").item = retained;
            if self.state == PlaybackStatus::Paused {
                self.pause(false).await?;
            }
            return Ok(None);
        }
        Ok(update.start.then(|| LoadOptions {
            history_shuffle_anchor: update.shuffle_anchor,
            ..LoadOptions::default()
        }))
    }

    fn same_history_row_online_binding(
        &self,
        loaded: &super::state::Loaded,
        selected: &crate::playback::QueueEntry,
    ) -> bool {
        use crate::playback::engine::MediaInput;
        let before = &loaded.item.resource;
        let after = &selected.resource;
        let online = |resource: &serde_json::Value| {
            super::quality::online_resource(resource)
                && [
                    "cloud_song_id",
                    "download_task_id",
                    "download_ledger",
                    "native_open_file",
                ]
                .iter()
                .all(|field| resource[*field].is_null())
        };
        if !online(before)
            || !online(after)
            || !matches!(loaded.source.input, MediaInput::Url(_))
            || !self
                .resolver
                .same_online_binding(before, after, &self.token)
        {
            return false;
        }
        if before["voice_id"].is_null() {
            // A changed restriction must be authorized again; stale
            // metadata alone does not force a new decoder.
            return after["privilege"]["pl"].as_i64().is_some_and(|pl| pl > 0)
                || before["privilege"] == after["privilege"];
        }
        let old_voice = &before["voice"];
        let new_voice = &after["voice"];
        let old_radio = &old_voice["audio"];
        let new_radio = &new_voice["audio"];
        !old_radio["id"].is_null()
            && old_radio["id"] == new_radio["id"]
            && old_radio["feeInfo"] == new_radio["feeInfo"]
            && [
                "feeInfo",
                "displayStatus",
                "disPlayStatus",
                "grayDisplay",
                "underShelf",
            ]
            .iter()
            .all(|field| old_voice[*field] == new_voice[*field])
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
        self.update_queue_inner(value, reply, None, true, None, None)
            .await
    }

    pub(super) async fn update_queue_with_source(
        &mut self,
        value: super::QueueUpdate,
        reply: Option<&Response>,
        first_source: Option<std::sync::Arc<crate::playback::source::ResolvedSource>>,
    ) -> Result<Option<LoadOptions>> {
        self.update_queue_inner(value, reply, first_source, false, None, None)
            .await
    }

    pub(super) async fn apply_scoped_podcast_queue(
        &mut self,
        value: super::QueueUpdate,
        first_source: Arc<ResolvedSource>,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
        liked_visit: Option<&LikedPageQueueFence>,
    ) -> Result<Option<LoadOptions>> {
        self.update_queue_inner(
            value,
            Some(reply),
            Some(first_source),
            false,
            Some(authority),
            liked_visit,
        )
        .await
    }

    pub(super) fn queue_scoped_podcast(
        &mut self,
        value: super::QueueUpdate,
        reply: &Response,
        authority: &crate::playback::engine::Cancellation,
        liked_visit: &LikedPageQueueFence,
    ) -> Result<()> {
        value.queue.validate()?;
        if self.queue.is_empty()
            || self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || value.queue.fm_source.is_some()
            || value.queue.intelligence.is_some()
            || value.transport.is_some()
            || value.queue.mode != self.queue.mode()
            || value.queue.selected() != self.queue.selected()
            || reply.is_closed()
        {
            return Err(Error::StaleResponse);
        }
        // Queue-only Next must not reload even when the incumbent is still
        // opening. The general authority may lock the account; only the
        // native visit fence can span the account-guarded queue CAS.
        authority.check()?;
        self.install_liked_page_queue(value.queue, liked_visit, Some(reply))
    }

    fn install_liked_page_queue(
        &mut self,
        candidate: crate::playback::QueueSnapshot,
        visit: &LikedPageQueueFence,
        reply: Option<&Response>,
    ) -> Result<()> {
        self.session.with_token(&self.token, |owner| {
            visit.with_active(&self.token, || {
                if reply.is_some_and(Response::is_closed) {
                    return Err(Error::StaleResponse);
                }
                self.queue.install_liked_page(candidate, owner, visit)
            })
        })
    }

    pub(super) fn prepare_liked_page_toggle(
        &mut self,
        mode: Option<crate::playback::PlaybackMode>,
        visit: &LikedPageQueueFence,
        reply: &Response,
    ) -> Result<()> {
        self.session.with_token(&self.token, |owner| {
            visit.with_active(&self.token, || {
                if reply.is_closed() {
                    return Err(Error::StaleResponse);
                }
                self.queue.set_liked_page_mode(mode, owner, visit)
            })
        })
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
        self.update_queue_inner(
            value,
            Some(reply),
            first_source,
            !start,
            Some(authority),
            None,
        )
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
            .update_queue_inner(
                value,
                Some(reply),
                first_source,
                false,
                Some(authority),
                None,
            )
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
        liked_visit: Option<&LikedPageQueueFence>,
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
        if let Some(visit) = liked_visit {
            self.install_liked_page_queue(value.queue, visit, reply)?;
        } else if insert_next || first_source.is_some() || authority.is_some() {
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
