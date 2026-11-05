use super::{
    PlaybackCommand, PlaybackSpeedScope, PlaybackStatus, Reply, Request, Response,
    load::{LoadOptions, LoadSource},
    state::Actor,
};
use crate::{
    Error, Result,
    playback::{Step, failure::PlaybackFailure},
};

impl Actor {
    pub async fn request(&mut self, request: Request) {
        if request.reply.is_closed() {
            return;
        }
        if let Err(error) = self.rebind().await {
            self.reply(request.reply, Err(error));
            return;
        }
        if request.token != self.token {
            self.reply(request.reply, Err(Error::StaleResponse));
            return;
        }
        if let Err(error) = self.sync_queue().await {
            self.reply(request.reply, Err(error));
            return;
        }
        if let Some((expected, cancel)) = &request.guard
            && (*expected != self.snapshot().fence() || cancel.is_cancelled())
        {
            self.reply(request.reply, Err(Error::StaleResponse));
            return;
        }
        if matches!(
            request.command,
            PlaybackCommand::UseMissingDownloadOnline { .. }
                | PlaybackCommand::ReplaceListenedBook { .. }
                | PlaybackCommand::ReplaceFollowLatest { .. }
                | PlaybackCommand::ReplaceProfileRankPlaylist { .. }
                | PlaybackCommand::MoveDisplayed(_)
                | PlaybackCommand::RetainCurrentQueueAndPlay(_)
        ) && request.guard.is_none()
        {
            self.reply(request.reply, Err(Error::StaleResponse));
            return;
        }
        if matches!(request.command, PlaybackCommand::Queue) {
            let _ = request.reply.send(Ok(Reply {
                state: self.snapshot(),
                queue: Some(self.queue.snapshot()),
                queue_more: self.queue_more_view(),
            }));
            return;
        }
        let more = match &request.command {
            PlaybackCommand::QueueMoreOpened { expected_revision } => {
                Some(self.queue_more_action(*expected_revision, None))
            }
            PlaybackCommand::QueueMoreToggle {
                expected_revision,
                enabled,
            } => Some(self.queue_more_action(*expected_revision, Some(*enabled))),
            _ => None,
        };
        if let Some(more) = more {
            self.reply(request.reply, more);
            return;
        }
        if let PlaybackCommand::SkipStartup(expected) = &request.command {
            if expected == &self.snapshot().fence() {
                self.startup_pending = false;
                self.startup_autoplay = false;
            }
            self.reply(request.reply, Ok(()));
            return;
        }
        if let PlaybackCommand::Startup(input) = &request.command {
            if !self.startup_pending {
                self.reply(request.reply, Ok(()));
                return;
            }
            if input.expected != self.snapshot().fence() {
                self.reply(request.reply, Err(Error::QueueChanged));
                return;
            }
        } else if !matches!(
            request.command,
            PlaybackCommand::Snapshot
                | PlaybackCommand::SyncQueue
                | PlaybackCommand::Volume(_)
                | PlaybackCommand::Speed(_)
                | PlaybackCommand::SpeedFor { .. }
                | PlaybackCommand::Equalizer { .. }
                | PlaybackCommand::SaveCustomEqualizer { .. }
                | PlaybackCommand::RetryEqualizer
                | PlaybackCommand::RepairEqualizer
                | PlaybackCommand::Loudness { .. }
                | PlaybackCommand::RetryLoudness
                | PlaybackCommand::RepairLoudness
                | PlaybackCommand::RepairQuality
        ) {
            // A user/remote transport intent wins over a delayed UI connect.
            self.startup_pending = false;
            self.startup_autoplay = false;
        }
        if self.queue.is_fm()
            && matches!(
                request.command,
                PlaybackCommand::Next
                    | PlaybackCommand::Previous
                    | PlaybackCommand::StepPreservingState(_)
                    | PlaybackCommand::Select(_)
                    | PlaybackCommand::Remove(_)
                    | PlaybackCommand::RemoveAndResume { .. }
                    | PlaybackCommand::Reorder(_)
                    | PlaybackCommand::MoveDisplayed(_)
                    | PlaybackCommand::Mode(_)
                    | PlaybackCommand::Append { .. }
            )
        {
            self.reply(
                request.reply,
                Err(Error::State("FM queue requires its coordinator")),
            );
            return;
        }
        if let PlaybackCommand::ApplyRemote(value) = &request.command
            && value.expected != self.snapshot().fence()
        {
            self.reply(request.reply, Err(Error::QueueChanged));
            return;
        }
        if let PlaybackCommand::UpdateQueue(value)
        | PlaybackCommand::PlaySearchCatalog(Some(value))
        | PlaybackCommand::UpdateZoneFm { update: value, .. } = &request.command
            && value.expected != self.snapshot().fence()
        {
            self.reply(request.reply, Err(Error::QueueChanged));
            return;
        }
        if let PlaybackCommand::RestoreFm(value) = &request.command
            && value.expected != self.snapshot().fence()
        {
            self.reply(request.reply, Err(Error::QueueChanged));
            return;
        }
        if let PlaybackCommand::Intelligence(value) = &request.command
            && value.expected != self.snapshot().fence()
        {
            self.reply(request.reply, Err(Error::QueueChanged));
            return;
        }
        if !matches!(
            request.command,
            PlaybackCommand::Snapshot
                | PlaybackCommand::Queue
                | PlaybackCommand::SyncQueue
                | PlaybackCommand::Volume(_)
                | PlaybackCommand::Speed(_)
                | PlaybackCommand::SpeedFor { .. }
                | PlaybackCommand::Equalizer { .. }
                | PlaybackCommand::SaveCustomEqualizer { .. }
                | PlaybackCommand::RetryEqualizer
                | PlaybackCommand::RepairEqualizer
                | PlaybackCommand::Loudness { .. }
                | PlaybackCommand::RetryLoudness
                | PlaybackCommand::RepairLoudness
                | PlaybackCommand::RepairQuality
        ) {
            let Some(revision) = self.control_revision.checked_add(1) else {
                self.reply(
                    request.reply,
                    Err(Error::State("playback control revision exhausted")),
                );
                return;
            };
            self.control_revision = revision;
        }
        if let PlaybackCommand::Quality(level) = request.command {
            self.quality(request.id, level, None, request.reply);
            return;
        }
        if let PlaybackCommand::TrialQuality { right, operation } = request.command {
            self.trial_quality(request.id, right, operation, request.reply);
            return;
        }
        let mut options = None;
        let mut resume_after_removal = None;
        if matches!(
            request.command,
            PlaybackCommand::Replace { .. }
                | PlaybackCommand::ReplaceListenedBook { .. }
                | PlaybackCommand::ReplaceFollowLatest { .. }
                | PlaybackCommand::ReplaceProfileRankPlaylist { .. }
                | PlaybackCommand::Select(_)
                | PlaybackCommand::Next
                | PlaybackCommand::Previous
                | PlaybackCommand::StepPreservingState(_)
                | PlaybackCommand::Remove(_)
                | PlaybackCommand::RemoveAndResume { .. }
        ) && let Err(error) = self.save_position()
        {
            self.reply(request.reply, Err(error));
            return;
        }
        let supplied_voice_checkpoint = matches!(
            &request.command,
            PlaybackCommand::ReplaceListenedBook { .. }
        );
        let relay_hint = crate::relay::publisher::player::Hint::from_command(&request.command);
        let prefer_search_download =
            matches!(&request.command, PlaybackCommand::PlaySearchCatalog(_));
        let result = match request.command {
            PlaybackCommand::Startup(input) => {
                self.startup_restore(input).map(|value| options = value)
            }
            PlaybackCommand::Equalizer {
                expected_revision,
                settings,
                preset,
            } => {
                self.equalizer_change(expected_revision, settings, preset)
                    .await
            }
            PlaybackCommand::SaveCustomEqualizer { expected_revision } => {
                self.equalizer_save_custom(expected_revision)
            }
            PlaybackCommand::RetryEqualizer => self.equalizer_retry().await,
            PlaybackCommand::RepairEqualizer => self.equalizer_repair().await,
            PlaybackCommand::Loudness {
                expected_revision,
                enabled,
            } => self.loudness_change(&expected_revision, enabled).await,
            PlaybackCommand::RetryLoudness => self.loudness_retry().await,
            PlaybackCommand::RepairLoudness => self.loudness_repair().await,
            PlaybackCommand::RepairQuality => self.quality_repair(),
            PlaybackCommand::UpdateQueue(value)
            | PlaybackCommand::PlaySearchCatalog(Some(value)) => self
                .update_queue(value, Some(&request.reply))
                .await
                .map(|value| options = value),
            PlaybackCommand::RetainCurrentQueueAndPlay(queue) => self
                .retain_current_queue_and_play(queue)
                .await
                .map(|value| options = value),
            PlaybackCommand::UpdateZoneFm {
                update,
                retain_paused,
            } => self
                .update_zone_fm(update, retain_paused)
                .await
                .map(|value| options = value),
            PlaybackCommand::ReplaceFollowLatest {
                resources,
                first_download,
            } => self
                .replace_follow_latest(resources, first_download)
                .await
                .map(|value| options = value),
            PlaybackCommand::ReplaceProfileRankPlaylist { resources, source } => self
                .replace_collection_preserving_song(resources, source, None)
                .await
                .map(|value| options = value),
            PlaybackCommand::RestoreFm(value) => {
                self.restore_fm(value).await.map(|value| options = value)
            }
            PlaybackCommand::Intelligence(value) => self
                .intelligence(value.action)
                .await
                .map(|value| options = value),
            PlaybackCommand::ApplyRemote(value) => self
                .apply_remote(value, Some(&request.reply))
                .await
                .map(|value| options = value),
            PlaybackCommand::Replace {
                mut resources,
                index,
                source,
            }
            | PlaybackCommand::ReplaceListenedBook {
                mut resources,
                index,
                source,
            } => {
                self.stamp_quality(&mut resources);
                self.queue.replace(resources, index, source).map(|_| {
                    if !self.queue.is_empty() {
                        options = Some(LoadOptions {
                            supplied_voice_checkpoint,
                            ..LoadOptions::default()
                        });
                    }
                })
            }
            PlaybackCommand::Append {
                mut resources,
                source,
            } => {
                self.stamp_quality(&mut resources);
                self.queue.append(resources, source).map(|_| ())
            }
            PlaybackCommand::Select(id) => {
                let selected = if request.verified_skip_selection {
                    match &request.guard {
                        Some((_, authority)) if !authority.is_cancelled() => self
                            .session
                            .with_token(&self.token, |_| self.queue.select(&id)),
                        _ => Err(Error::StaleResponse),
                    }
                } else {
                    self.queue.select(&id)
                };
                selected.map(|_| {
                    // Queue selection has committed. M308's notice describes
                    // this transition; URL/decoder success does not emit it.
                    if request.verified_skip_selection {
                        self.publish_ordinary_skip();
                    }
                    options = Some(LoadOptions::default());
                })
            }
            PlaybackCommand::Remove(ids) => self.queue.remove(&ids),
            PlaybackCommand::RemoveAndResume {
                entry_ids,
                audio_owner,
            } => {
                let removing_paused_current = self.state == PlaybackStatus::Paused
                    && self.queue.intelligence_context().is_none()
                    && self.loaded.as_ref().is_some_and(|loaded| {
                        entry_ids.contains(&loaded.item.entry_id)
                            && self.queue.selected().as_ref() == Some(&loaded.item)
                    });
                self.queue.remove(&entry_ids).map(|_| {
                    if removing_paused_current {
                        resume_after_removal = Some(audio_owner);
                    }
                })
            }
            PlaybackCommand::Reorder(ids) => self.queue.reorder(&ids),
            PlaybackCommand::MoveDisplayed(movement) => self.queue.reorder_displayed(&movement),
            PlaybackCommand::Mode(mode) => {
                if let Some(context) = self
                    .queue
                    .intelligence_context()
                    .filter(|c| c.active())
                    .cloned()
                {
                    self.intelligence(crate::playback::intelligence::IntelligenceAction::Leave {
                        session_id: context.id,
                        mode,
                        retain_missing_current: false,
                    })
                    .await
                    .map(|value| options = value)
                } else {
                    self.queue.set_mode(mode)
                }
            }
            PlaybackCommand::Play
            | PlaybackCommand::PlayFromStart
            | PlaybackCommand::PlaySearchCatalog(None) => {
                if self.state == PlaybackStatus::Paused || self.pending.is_some() {
                    self.pause(false).await
                } else if self.state.active() {
                    Ok(())
                } else {
                    options = Some(self.resume_interrupted_song().unwrap_or_else(|| {
                        if matches!(request.command, PlaybackCommand::PlayFromStart) {
                            LoadOptions {
                                position: Some(0.0),
                                relative: true,
                                ..Default::default()
                            }
                        } else {
                            LoadOptions::default()
                        }
                    }));
                    Ok(())
                }
            }
            PlaybackCommand::Pause(paused) => self.pause(paused).await,
            PlaybackCommand::Seek(seconds) => {
                self.seek(
                    seconds,
                    &request.reply,
                    request.guard.map(|(_, cancel)| cancel),
                )
                .await
            }
            PlaybackCommand::SeekDuringLoad(seconds) => {
                if let Some((_, authority)) = request.guard {
                    self.seek_during_load(seconds, authority, &request.reply)
                        .await
                } else {
                    Err(Error::InvalidInput(
                        "loading seek requires a playback fence",
                    ))
                }
            }
            PlaybackCommand::Volume(value) => self.preference(value, true, None).await,
            PlaybackCommand::Speed(value) => self.preference(value, false, None).await,
            PlaybackCommand::SpeedFor { scope, value } => {
                self.preference(value, false, Some(scope)).await
            }
            PlaybackCommand::Next
            | PlaybackCommand::Previous
            | PlaybackCommand::StepPreservingState(_) => {
                let (step, preserve) = match request.command {
                    PlaybackCommand::Next => (Step::Next, false),
                    PlaybackCommand::Previous => (Step::Previous, false),
                    PlaybackCommand::StepPreservingState(step) => (step, true),
                    _ => unreachable!("step command matched"),
                };
                match self.queue.advance(step, false) {
                    Ok(Some(_)) => {
                        if preserve && !self.state.active() && self.pending.is_none() {
                            // Clear old decoder ownership before queue synchronization,
                            // which must not reload a stopped selection on its own.
                            self.stop(false).await
                        } else {
                            options = Some(LoadOptions {
                                paused: preserve
                                    && (self.state == PlaybackStatus::Paused
                                        || self.pending.as_ref().is_some_and(|p| p.paused)),
                                ..Default::default()
                            });
                            Ok(())
                        }
                    }
                    Ok(None) => {
                        self.depart_podcast(false);
                        self.stop(true).await
                    }
                    Err(error) => Err(error),
                }
            }
            PlaybackCommand::Stop => {
                self.depart_podcast(false);
                self.stop(true).await
            }
            PlaybackCommand::Suspend => {
                if self.pending.is_some() || self.quality_task.is_some() {
                    self.stop(true).await
                } else if self.state.active() && self.loaded.is_some() {
                    self.pause(true).await
                } else {
                    Ok(())
                }
            }
            PlaybackCommand::Restore => match self.restore_options() {
                Ok(value) => {
                    options = value;
                    Ok(())
                }
                Err(error) => Err(error),
            },
            PlaybackCommand::RefreshSource => {
                let selected = self.queue.selected();
                if let Some(item) = selected {
                    let id = item.resource["id"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| item.resource["id"].as_u64().map(|n| n.to_string()));
                    match self.resolver.invalidate(id.as_deref()) {
                        Ok(()) => {
                            let retry = self
                                .source_retry
                                .as_ref()
                                .filter(|retry| retry.item == item);
                            let position =
                                self.position.or_else(|| retry.map(|retry| retry.position));
                            options = Some(LoadOptions {
                                paused: self.state == PlaybackStatus::Paused
                                    || self.pending.as_ref().is_some_and(|p| p.paused)
                                    || (self.pending.is_none()
                                        && retry.is_some_and(|retry| retry.paused)),
                                position,
                                recover: true,
                                retain_source_retry: position.is_some(),
                                ..Default::default()
                            });
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    Err(Error::State("queue is empty"))
                }
            }
            PlaybackCommand::UseMissingDownloadOnline {
                resource,
                source,
                preference_revision,
            } => {
                if self.state != PlaybackStatus::Error
                    || self.queue.is_fm()
                    || self.queue.intelligence_context().is_some()
                    || !matches!(self.error, Some(PlaybackFailure::DownloadedFileMissing))
                    || self.quality_preferences.preferences.revision != preference_revision
                {
                    Err(Error::StaleResponse)
                } else if let Some(entry) = self.queue.selected().filter(|entry| {
                    entry.resource["download_task_id"].is_string()
                        && entry.resource["id"] == resource["id"]
                }) {
                    self.session
                        .with_token(&self.token, |_| {
                            self.queue.update_resource(&entry.entry_id, resource)
                        })
                        .map(|_| {
                            options = Some(LoadOptions {
                                position: Some(0.0),
                                relative: true,
                                source: LoadSource::Retained(source),
                                ..Default::default()
                            });
                        })
                } else {
                    Err(Error::StaleResponse)
                }
            }
            PlaybackCommand::Snapshot | PlaybackCommand::SyncQueue => Ok(()),
            PlaybackCommand::Quality(_)
            | PlaybackCommand::TrialQuality { .. }
            | PlaybackCommand::Queue
            | PlaybackCommand::QueueMoreOpened { .. }
            | PlaybackCommand::QueueMoreToggle { .. }
            | PlaybackCommand::SkipStartup(_) => unreachable!("handled above"),
        };
        if let Err(error) = result {
            self.reply(request.reply, Err(error));
            return;
        }
        self.relay_hint = relay_hint;
        if let Some(mut options) = options {
            options.prefer_search_download = prefer_search_download;
            if let Err(error) = self.begin(options, request.id, Some(request.reply)).await {
                self.record_failure(&error);
            }
        } else {
            let result = self.sync_queue_after_edit(resume_after_removal).await;
            self.reply(request.reply, result);
        }
    }
    async fn retain_current_queue_and_play(
        &mut self,
        queue: crate::playback::QueueSnapshot,
    ) -> Result<Option<LoadOptions>> {
        let selected = self.queue.selected();
        // This command authorizes continuing the current decoder only. It
        // cannot replace the media locator, rights, identity or queue family.
        if self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
            || queue.fm_source.is_some()
            || queue.intelligence.is_some()
            || selected.is_none()
            || queue.selected() != selected
        {
            return Err(Error::StaleResponse);
        }
        self.save_position()?;
        self.queue.install(queue)?;
        if let Some(pending) = &self.pending {
            if pending.paused {
                self.pause(false).await?;
            }
            return Ok(None);
        }
        if self.state == PlaybackStatus::Paused {
            self.pause(false).await?;
            return Ok(None);
        }
        if self.state.active() {
            return Ok(None);
        }
        Ok(Some(self.resume_interrupted_song().unwrap_or_default()))
    }

    fn resume_interrupted_song(&self) -> Option<LoadOptions> {
        let source = match self.state {
            // Only an unsolicited native stop leaves a retry point in Idle.
            // It doesn't invalidate a still-authorized URL.
            PlaybackStatus::Idle => LoadSource::CurrentPreference,
            PlaybackStatus::Error
                if matches!(
                    self.error,
                    Some(
                        PlaybackFailure::Engine
                            | PlaybackFailure::OpenMedia
                            | PlaybackFailure::Output
                            | PlaybackFailure::Network
                            | PlaybackFailure::SourceServiceTemporary
                    )
                ) =>
            {
                LoadSource::FreshOnline
            }
            _ => return None,
        };
        let retry = self.source_retry.as_ref()?;
        let can_resume = if self.state == PlaybackStatus::Idle {
            self.independent_transport(&retry.item)
        } else {
            self.ordinary_online_song(&retry.item)
        };
        if self.queue.selected().as_ref() != Some(&retry.item) || !can_resume {
            return None;
        }
        // Play is explicit consent to resume, even if the interrupted load was
        // paused. Retry keeps the previous pause intent instead.
        Some(LoadOptions {
            position: Some(retry.position),
            recover: true,
            retain_source_retry: true,
            restart_at_end: self.state == PlaybackStatus::Idle,
            source,
            ..Default::default()
        })
    }
    fn require_loaded(&self) -> Result<()> {
        if self.state.active() && self.loaded.is_some() {
            Ok(())
        } else {
            Err(Error::State("nothing is loaded"))
        }
    }
    pub(super) async fn pause(&mut self, paused: bool) -> Result<()> {
        if let Some(pending) = &mut self.pending {
            // A pause accepted during remote/ordinary decoding must determine
            // its first presentation state, rather than fail and later autoplay.
            pending.paused = paused;
            if let Some(retry) = &mut self.source_retry {
                retry.paused = paused;
            }
            self.power_pending();
            return Ok(());
        }
        self.require_loaded()?;
        let preserve_position = paused && self.state == PlaybackStatus::Paused;
        let observed = self
            .parametric_transport(paused, self.guard(), move |engine| {
                engine.pause(paused)?;
                Ok(engine.poll())
            })
            .await?;
        let processor_fault = observed
            .as_ref()
            .is_err_and(super::effect_runtime::is_processor_fault);
        let view = match self.recover_effect_poll(observed).await {
            Err(error) if processor_fault => {
                // The transport command already succeeded. If removal or
                // resampling fails, never keep the old pause state visible.
                self.stop_after_poll_failure(&error).await;
                return Err(error);
            }
            result => result?,
        };
        let loaded = self.loaded.as_ref().expect("checked");
        if let Some(position) = view.position_for(loaded.media.entry_id)
            && !self.paused_seek
        {
            let observed = loaded.timeline.from_media(position)?;
            if !preserve_position || !self.position.is_some_and(|p| observed < p) {
                self.position = Some(observed);
            }
        }
        // Poll consumes native events even when we only need the clock.
        // The regular tick still owns their handling for this load.
        self.deferred_events.extend(view.events);
        if !paused {
            self.paused_seek = false;
        }
        self.parametric.reset_activity();
        self.state = if paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        };
        self.persist_transport().await
    }
    async fn seek(
        &mut self,
        seconds: f64,
        reply: &Response,
        authority: Option<crate::playback::engine::Cancellation>,
    ) -> Result<()> {
        self.require_loaded()?;
        let loaded = self.loaded.as_ref().expect("checked");
        let media = loaded.timeline.to_media(seconds)?;
        let entry = loaded.media.entry_id;
        let local_equalizer = loaded.local_equalizer;
        let item = loaded.item.clone();
        let paused = self.state == PlaybackStatus::Paused;
        let cancel = self.guard();
        let cancel = match authority.filter(|_| local_equalizer) {
            Some(authority) => crate::playback::engine::Cancellation::new(move || {
                !cancel.is_cancelled() && !authority.is_cancelled()
            }),
            None => cancel,
        };
        let seek_cancel = cancel.clone();
        let result = super::engine(self.engine.clone(), cancel.clone(), move |engine| {
            engine.seek_with_receipt(media, entry, &seek_cancel)
        })
        .await;
        match result {
            Ok(Some(media)) => {
                let finish = async {
                    if reply.is_closed() {
                        return Err(Error::Cancelled);
                    }
                    self.queue.reload()?;
                    if self.queue.selected().as_ref() != Some(&item) {
                        return Err(Error::QueueChanged);
                    }
                    let replacement = media.entry_id;
                    super::engine(self.engine.clone(), cancel, move |engine| {
                        if engine.local_equalizer_entry() != Some(replacement) {
                            return Err(Error::StaleResponse);
                        }
                        // This unpause goes through native focus/output.
                        engine.pause(paused)
                    })
                    .await
                }
                .await;
                if let Err(error) = finish {
                    self.stop_after_poll_failure(&error).await;
                    return Err(error);
                }
                self.loaded.as_mut().expect("checked").media = media;
                // The new entry has loaded, but only its matching native
                // poll can report that startup synchronization is complete.
                self.native_seeking = true;
                self.deferred_events.clear();
            }
            Ok(None) => {}
            Err(error) => {
                if local_equalizer {
                    self.stop_after_poll_failure(&error).await;
                }
                return Err(error);
            }
        }
        self.parametric.reset_activity();
        self.position = Some(seconds);
        self.paused_seek = self.state == PlaybackStatus::Paused;
        self.buffered_through = None;
        self.seek_revision = self.control_revision;
        self.persist_transport().await
    }
    async fn seek_during_load(
        &mut self,
        seconds: f64,
        authority: crate::playback::engine::Cancellation,
        reply: &Response,
    ) -> Result<()> {
        if self.queue.is_fm() {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        let Some(pending) = &mut self.pending else {
            // The decoder may complete after the client captured the fence.
            return self.seek(seconds, reply, Some(authority)).await;
        };
        let timeline = pending
            .timeline
            .filter(|timeline| timeline.display_end().is_some())
            .ok_or(Error::State("loading source has no playable duration"))?;
        timeline.to_media(seconds)?;
        pending.seek = Some((seconds, self.control_revision, authority));
        Ok(())
    }
    pub(super) async fn persist_transport(&mut self) -> Result<()> {
        if let Err(error) = self.save_position() {
            let _ = self.stop(false).await;
            self.state = PlaybackStatus::Error;
            self.record_failure(&error);
            return Err(error);
        }
        Ok(())
    }
    async fn preference(
        &mut self,
        value: f64,
        volume: bool,
        speed_scope: Option<PlaybackSpeedScope>,
    ) -> Result<()> {
        if !value.is_finite()
            || (volume && !(0.0..=100.0).contains(&value))
            || (!volume && !(0.25..=4.0).contains(&value))
        {
            return Err(Error::InvalidInput("invalid player preference"));
        }
        let before = self.prefs;
        let mut next = before;
        let current_scope = PlaybackSpeedScope::for_entry(self.queue.selected().as_ref());
        let scope = speed_scope.unwrap_or(current_scope);
        let apply_engine = self.state.active() && (volume || scope == current_scope);
        if volume {
            next.volume = value;
            // Keep restoration native and durable for UI, media keys and MPRIS.
            // Old zero-volume preferences without a saved value restore to 10%.
            if value > 0.0 {
                next.unmute_volume = value;
            } else if before.volume > 0.0 {
                next.unmute_volume = before.volume;
            }
        } else {
            next.set_speed(scope, value);
        }
        if apply_engine {
            super::engine(self.engine.clone(), self.guard(), move |engine| {
                if volume {
                    engine.volume(value)
                } else {
                    engine.speed(value)
                }
            })
            .await?;
        }
        if let Err(error) = self.store.put(
            self.queue.owner(),
            "playback",
            "preferences",
            &serde_json::to_value(next)?,
        ) {
            if apply_engine {
                super::engine(self.engine.clone(), self.guard(), move |engine| {
                    if volume {
                        engine.volume(before.volume)
                    } else {
                        engine.speed(before.speed(scope))
                    }
                })
                .await?;
            }
            return Err(error);
        }
        self.prefs = next;
        Ok(())
    }
    fn restore_options(&self) -> Result<Option<LoadOptions>> {
        let Some(selected) = self.queue.selected() else {
            return Ok(None);
        };
        let saved = self.store.get(self.queue.owner(), "playback", "position")?;
        let matched = saved
            .as_ref()
            .filter(|v| v["entry_id"].as_str() == Some(&selected.entry_id));
        let ended = matched.is_some_and(|v| v["state"] == "ended");
        let position = if ended {
            None
        } else {
            matched.and_then(|v| v["seconds"].as_f64())
        };
        Ok(Some(LoadOptions {
            paused: true,
            position,
            recover: true,
            relative: matched.is_some_and(|v| v["timeline_version"] != 1),
            ..Default::default()
        }))
    }
}
