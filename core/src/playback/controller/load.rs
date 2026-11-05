use super::{
    Message, PlaybackSpeedScope, PlaybackStatus, Response,
    state::{Actor, Loaded, Pending, SourceRetryPoint},
};
use crate::{
    Error, Result,
    effects::equalizer::EqualizerStatus,
    playback::{
        QueueEntry, Timeline, TimelineMode,
        engine::{Cancellation, LoadRequest, LoadedMedia, MediaInput},
        failure::PlaybackFailure,
        source::{ResolvedSource, StreamingSelection},
    },
};
use std::sync::Arc;

#[derive(Default)]
pub(super) enum LoadSource {
    #[default]
    CurrentPreference,
    Retained(Arc<ResolvedSource>),
    RenewIfExpired(Arc<ResolvedSource>),
    FreshOnline,
}

#[derive(Default)]
pub(super) struct LoadOptions {
    pub paused: bool,
    pub position: Option<f64>,
    pub recover: bool,
    pub retain_source_retry: bool,
    pub relative: bool,
    pub restart_at_end: bool,
    pub supplied_voice_checkpoint: bool,
    pub source: LoadSource,
}
pub(super) struct Prepared {
    pub item: QueueEntry,
    pub source: Arc<ResolvedSource>,
    pub media: LoadedMedia,
    pub local_equalizer: bool,
    pub timeline: Timeline,
    pub position: f64,
    pub reset: bool,
    stock_fallback: Option<Cancellation>,
    audiobook_skip: Option<crate::podcast::AudiobookIntervals>,
    audiobook_skip_sequence: u64,
    apply_audiobook_intro: bool,
}

fn with_stock_fallback(transport: Cancellation, fallback: Option<Cancellation>) -> Cancellation {
    match fallback {
        None => transport,
        Some(fallback) => {
            Cancellation::new(move || !transport.is_cancelled() && !fallback.is_cancelled())
        }
    }
}

fn loading_timeline(source: &ResolvedSource, item: &QueueEntry, timeline: Timeline) -> Timeline {
    if timeline.display_end().is_some() {
        return timeline;
    }
    let resource = &item.resource;
    let seconds = resource["metadata"]["duration"].as_f64();
    let millis = resource["dt"]
        .as_f64()
        .or_else(|| resource["song"]["dt"].as_f64());
    seconds
        .or_else(|| millis.map(|ms| ms / 1000.0))
        .and_then(|duration| source.timeline(Some(duration)).ok())
        .unwrap_or(timeline)
}

impl Actor {
    pub(super) fn independent_transport(&self, item: &QueueEntry) -> bool {
        !self.queue.is_fm()
            && self.queue.intelligence_context().is_none()
            && item.source["kind"] != "listen_together"
            && item.resource["playback_constraint"].is_null()
    }

    pub(super) fn ordinary_online_song(&self, item: &QueueEntry) -> bool {
        self.independent_transport(item)
            && item.resource["cloud_song_id"].is_null()
            && item.resource["download_task_id"].is_null()
            && super::quality::song_resource(&item.resource)
    }

    pub async fn begin(
        &mut self,
        options: LoadOptions,
        id: String,
        mut reply: Option<Response>,
    ) -> Result<()> {
        let result = self.prepare_begin(options, id, &mut reply).await;
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let error = Arc::new(error);
                if let Some(reply) = reply {
                    let _ = reply.send(Err(Error::Shared(error.clone())));
                }
                Err(Error::Shared(error))
            }
        }
    }
    async fn prepare_begin(
        &mut self,
        options: LoadOptions,
        id: String,
        reply: &mut Option<Response>,
    ) -> Result<()> {
        let mut item = self
            .queue
            .selected()
            .ok_or(Error::State("queue is empty"))?;
        // Recovery keeps the request for the interrupted source. Only ordinary
        // new loads resolve the latest account listening choice.
        if matches!(&options.source, LoadSource::CurrentPreference) {
            self.apply_quality_preference(&mut item.resource);
            if self.queue.selected().as_ref() != Some(&item) {
                self.queue
                    .update_resource(&item.entry_id, item.resource.clone())?;
            }
        }
        let constraint = crate::playback::PlaybackConstraint::from_resource(&item.resource)?;
        if matches!(
            constraint,
            Some(crate::playback::PlaybackConstraint::Blocked)
        ) {
            return Err(Error::State("resource is not jointly playable"));
        }
        if options.position.is_some_and(|p| !p.is_finite() || p < 0.0) {
            return Err(Error::InvalidInput("invalid playback position"));
        }
        // Source refresh/quality reloads of the same selection keep the play
        // checkpoint. Selecting again or completing a loop creates a new one.
        let history = self
            .loaded
            .as_ref()
            .filter(|l| {
                options.recover && options.position.is_some() && l.item.entry_id == item.entry_id
            })
            .map(|l| l.history.clone())
            .or_else(|| {
                self.source_retry
                    .as_ref()
                    .filter(|retry| options.retain_source_retry && retry.item == item)
                    .map(|retry| retry.history.clone())
            })
            .unwrap_or_default();
        if history.played_at.is_none() {
            self.history_error = None;
        }
        let same_play = self.loaded.as_ref().is_some_and(|loaded| {
            options.recover && options.position.is_some() && loaded.item.entry_id == item.entry_id
        });
        let podcast_progress = if same_play {
            self.loaded
                .as_mut()
                .and_then(|loaded| loaded.podcast_progress.take())
        } else {
            self.depart_podcast(false);
            None
        };
        self.cancel_load().await?;
        self.source_retry = options
            .position
            .filter(|_| options.retain_source_retry)
            .map(|position| SourceRetryPoint {
                item: item.clone(),
                position,
                paused: options.paused,
                history: history.clone(),
            });
        self.loaded = None;
        self.loudness_forget_track();
        self.position = None;
        self.buffered_through = None;
        self.error = None;
        self.reset = false;
        self.parametric.discard_paused_device();
        let local_equalizer = (item.resource.get("native_open_file").is_some()
            && self.independent_transport(&item)
            && constraint.is_none()
            && self.parametric.admits_local_equalizer()
            && !self.equalizer.damaged
            && self.equalizer.preferences.settings.local_graph_requested())
        .then_some(self.equalizer.preferences.settings);
        if local_equalizer.is_some() {
            // If preparation displaced a stock filter and then failed, the
            // next accepted source still reconciles the durable EQ controls.
            self.local_equalizer_needs_sync = true;
        }
        self.play_id = Some(uuid::Uuid::new_v4().simple().to_string());
        self.state = PlaybackStatus::Preparing;
        let generation = self.generation();
        let cancel = self.guard();
        self.cancel = cancel.clone();
        self.pending = Some(Pending {
            history,
            podcast_progress,
            id,
            item: item.clone(),
            reply: reply.take(),
            paused: options.paused,
            timeline: None,
            authorized_position: None,
            seek: None,
        });
        self.power_pending();
        let resolver = self.resolver.clone();
        let session = self.session.clone();
        let audiobook_skip_sequence = self.audiobook_skip_sequence;
        let apply_audiobook_intro = !same_play && item.source["scene"] != "playlistHandoff";
        let token = self.token.clone();
        let engine = self.engine.clone();
        let sender = self.sender.clone();
        let streaming_revision = resolver
            .streaming_cache
            .as_ref()
            .filter(|_| {
                self.ordinary_online_song(&item)
                    && token.uid.is_some()
                    && matches!(&options.source, LoadSource::CurrentPreference)
            })
            .and_then(|cache| cache.store.revision().ok());
        self.load_task = Some(tokio::spawn(async move {
            let result = async {
                let streaming_revision =
                    super::engine(engine.clone(), cancel.clone(), move |engine| {
                        engine.stop()?;
                        // Discard the previous track's gain before priming a new decoder.
                        engine.reset_track_loudness()?;
                        Ok(streaming_revision.filter(|_| engine.streaming_audio_available()))
                    })
                    .await?;
                cancel.check()?;
                let StreamingSelection {
                    source,
                    fresh_api,
                    stock_fallback,
                } = if let Some(revision) = streaming_revision {
                    resolver
                        .resolve_streaming_source(&item.resource, &token, revision)
                        .await?
                } else {
                    let source = match options.source {
                        LoadSource::RenewIfExpired(source)
                            if source.needs_renewal_for_reopen(resolver.now()) =>
                        {
                            // The retained entry supplies the original requested
                            // quality/identity. Its cache may already have been
                            // renewed by a single-track loop's prefetch.
                            resolver.resolve(&item.resource, &token).await?
                        }
                        LoadSource::RenewIfExpired(source) | LoadSource::Retained(source) => source,
                        LoadSource::CurrentPreference => {
                            resolver.resolve(&item.resource, &token).await?
                        }
                        LoadSource::FreshOnline => {
                            resolver
                                .resolve_fresh_online(&item.resource, &token)
                                .await?
                        }
                    };
                    StreamingSelection::stock(source)
                };
                cancel.check()?;
                let timeline = crate::playback::PlaybackConstraint::apply(
                    constraint.as_ref(),
                    source.timeline(None)?,
                )?;
                let snapshot = timeline.snapshot();
                let resumed = if options.position.is_none() {
                    resolver
                        .voice_resume_position(
                            &item.resource,
                            &token,
                            options.supplied_voice_checkpoint,
                        )
                        .await?
                } else {
                    None
                };
                cancel.check()?;
                let recover = options.recover || resumed.is_some();
                let mut position = options.position.or(resumed).unwrap_or(snapshot.start);
                if options.relative && options.position.is_some() {
                    position += snapshot.start;
                }
                let mut reset = false;
                if timeline.to_media(position).is_err() {
                    if !recover {
                        return Err(Error::InvalidInput("position outside authorized source"));
                    }
                    position = snapshot.start;
                    reset = true;
                }
                let mut load = LoadRequest::new(source.input.clone());
                load.start = snapshot.media_start;
                if matches!(snapshot.mode, TimelineMode::Cue | TimelineMode::Trial) {
                    load.end = timeline.media_end();
                }
                let _ = sender.send(Message::Loading {
                    generation,
                    timeline: loading_timeline(&source, &item, timeline),
                    authorized_position: position,
                });
                let stream_reader = if let (Some(revision), Some(api)) =
                    (streaming_revision, fresh_api)
                    && snapshot.mode == TimelineMode::Full
                    && snapshot.media_start == 0.0
                {
                    let prepared = resolver
                        .prepare_streaming_reader(
                            revision,
                            &source,
                            &item.resource,
                            token.clone(),
                            api.clone(),
                            cancel.clone(),
                        )
                        .await;
                    cancel.check()?;
                    resolver.validate_streaming_source(&token, &api)?;
                    match prepared {
                        Ok(reader) => reader,
                        Err(error @ (Error::Cancelled | Error::StaleResponse)) => {
                            return Err(error);
                        }
                        // Only cache/preflight failures fall back here: no
                        // media was selected or made audible through the new reader.
                        Err(_) => None,
                    }
                } else {
                    None
                };
                let load_cancel = with_stock_fallback(cancel.clone(), stock_fallback.clone());
                let (media, local_equalizer) =
                    super::engine(engine.clone(), load_cancel.clone(), move |engine| {
                        let media = if let Some(reader) = stream_reader {
                            engine.load_streaming_audio(reader, &load_cancel)?
                        } else if let Some(settings) = local_equalizer
                            && matches!(&load.source, MediaInput::NativeFile { .. })
                            && snapshot.mode == TimelineMode::Full
                            && engine.supports_local_equalizer()
                        {
                            engine.load_with_local_equalizer(load, settings, &load_cancel)?
                        } else {
                            engine.load(load, &load_cancel)?
                        };
                        let bound = engine.local_equalizer_entry() == Some(media.entry_id);
                        Ok((media, bound))
                    })
                    .await?;
                let timeline = crate::playback::PlaybackConstraint::apply(
                    constraint.as_ref(),
                    source.timeline(media.duration)?,
                )?;
                if timeline.to_media(position).is_err() {
                    if !recover {
                        return Err(Error::InvalidInput("position exceeds decoded duration"));
                    }
                    position = timeline.snapshot().start;
                    reset = true;
                }
                if options.restart_at_end
                    && timeline.display_end().is_some_and(|end| position >= end)
                {
                    position = timeline.snapshot().start;
                    reset = true;
                }
                let audiobook_skip =
                    super::audiobook_skip::prepare(&session, &token, &item, &source, timeline)
                        .await;
                cancel.check()?;
                Ok(Prepared {
                    item,
                    source,
                    media,
                    local_equalizer,
                    timeline,
                    position,
                    reset,
                    stock_fallback,
                    audiobook_skip,
                    audiobook_skip_sequence,
                    apply_audiobook_intro,
                })
            }
            .await;
            let _ = sender.send(Message::Loaded { generation, result });
        }));
        Ok(())
    }
    pub async fn loaded(&mut self, generation: u64, result: Result<Prepared>) {
        if generation != self.generation() {
            return;
        }
        // Another native queue writer may commit while the decoder is loading.
        // Reconcile before unpausing; a changed selection cancels this generation.
        if let Err(error) = self.sync_queue().await {
            let pending = self.pending.take();
            let _ = self.stop(false).await;
            self.state = PlaybackStatus::Error;
            self.record_failure(&error);
            if let Some(reply) = pending.and_then(|p| p.reply) {
                self.reply(reply, Err(error));
            }
            return;
        }
        if generation != self.generation() {
            return;
        }
        self.load_task = None;
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.reply.as_ref().is_some_and(|r| r.is_closed()) || self.cancel.is_cancelled() {
            if let Some(reply) = pending.reply {
                let _ = reply.send(Err(Error::Cancelled));
            }
            let _ = self.stop(false).await;
            return;
        }
        let mut applied_seek = None;
        let result = match result {
            Ok(mut prepared) => {
                if prepared.audiobook_skip_sequence == self.audiobook_skip_sequence
                    && prepared.apply_audiobook_intro
                    && let Some(intervals) = prepared.audiobook_skip
                    && let Some(duration) = prepared.timeline.display_end()
                {
                    let adjusted = intervals.start_position(prepared.position, duration);
                    if prepared.timeline.to_media(adjusted).is_ok() {
                        prepared.position = adjusted;
                    }
                }
                if prepared.item.entry_id == pending.item.entry_id
                    && prepared.item.resource == pending.item.resource
                {
                    // A coordinator may have restored only source provenance
                    // during this load. It cannot replace media/authorization.
                    prepared.item.source = pending.item.source.clone();
                }
                if let Err(error) = self.loudness_source(&prepared.source).await {
                    let _ = self.stop(false).await;
                    if let Some(reply) = pending.reply {
                        let _ = reply.send(Err(error));
                    }
                    return;
                }
                if prepared.local_equalizer && self.parametric.has_deferred_filter() {
                    match self.local_equalizer_stock(prepared.media.entry_id).await {
                        Ok(media) => {
                            prepared.media = media;
                            prepared.local_equalizer = false;
                        }
                        Err(error) => {
                            self.stop_after_poll_failure(&error).await;
                            if let Some(reply) = pending.reply {
                                let _ = reply.send(Err(error));
                            }
                            return;
                        }
                    }
                }
                if self.local_equalizer_needs_sync {
                    self.equalizer.status = EqualizerStatus::Pending;
                }
                if self.equalizer.status == EqualizerStatus::Pending
                    && let Err(error) = self.equalizer_apply().await
                {
                    let _ = self.stop(false).await;
                    if let Some(reply) = pending.reply {
                        let _ = reply.send(Err(error));
                    }
                    return;
                }
                self.local_equalizer_needs_sync = prepared.local_equalizer;
                if let Err(error) = self.parametric_finish_load(&prepared.source).await {
                    let _ = self.stop(false).await;
                    self.state = PlaybackStatus::Error;
                    self.record_failure(&error);
                    if let Some(reply) = pending.reply {
                        let _ = reply.send(Err(error));
                    }
                    return;
                }
                if let Some((position, revision, authority)) = pending.seek
                    && !authority.is_cancelled()
                {
                    if prepared.timeline.to_media(position).is_ok() {
                        prepared.position = position;
                        prepared.reset = false;
                        applied_seek = Some(revision);
                    } else {
                        // A catalog estimate is not decoder evidence. Retain
                        // the original authorized start when it disagrees.
                        prepared.reset = true;
                    }
                }
                let target = prepared.timeline.to_media(prepared.position);
                let prefs = self.prefs;
                let speed = prefs.speed(PlaybackSpeedScope::for_entry(Some(&prepared.item)));
                let paused = pending.paused;
                match target {
                    Ok(target) => {
                        let initial = prepared.timeline.snapshot().media_start;
                        let stock_fallback = prepared.stock_fallback.clone();
                        let transport =
                            with_stock_fallback(self.cancel.clone(), stock_fallback.clone());
                        let seek_cancel = transport.clone();
                        let entry = prepared.media.entry_id;
                        let local_equalizer = prepared.local_equalizer;
                        let result = self
                            .parametric_transport(paused, transport.clone(), move |engine| {
                                let replacement = if (target - initial).abs() > f64::EPSILON {
                                    engine.seek_with_receipt(target, entry, &seek_cancel)?
                                } else {
                                    None
                                };
                                engine.volume(prefs.volume)?;
                                engine.speed(speed)?;
                                if let Some(guard) = stock_fallback {
                                    guard.check()?;
                                }
                                if !local_equalizer {
                                    engine.pause(paused)?;
                                }
                                Ok(replacement)
                            })
                            .await;
                        async {
                            if let Some(media) = result? {
                                prepared.media = media;
                            }
                            self.queue.reload()?;
                            if self.queue.selected().as_ref() != Some(&prepared.item) {
                                return Err(Error::QueueChanged);
                            }
                            if local_equalizer {
                                if pending
                                    .reply
                                    .as_ref()
                                    .is_some_and(|reply| reply.is_closed())
                                {
                                    return Err(Error::Cancelled);
                                }
                                let entry = prepared.media.entry_id;
                                self.parametric_transport(
                                    paused,
                                    transport.clone(),
                                    move |engine| {
                                        if engine.local_equalizer_entry() != Some(entry) {
                                            return Err(Error::StaleResponse);
                                        }
                                        engine.pause(paused)
                                    },
                                )
                                .await?;
                            }
                            // M/308 obtains current persisted setting after
                            // native Play. A peer may have saved while loading.
                            self.equalizer_read()?;
                            let transition = self.equalizer_source_transition(&prepared.source);
                            if transition.is_some()
                                || self.equalizer.status == EqualizerStatus::Pending
                            {
                                self.equalizer_apply_configuration(
                                    transition.unwrap_or(false),
                                    transport.clone(),
                                )
                                .await?;
                                // Applying to the engine can wait. Do not let a
                                // superseded selection publish or consume Ge.
                                transport.check()?;
                                if pending
                                    .reply
                                    .as_ref()
                                    .is_some_and(|reply| reply.is_closed())
                                {
                                    return Err(Error::Cancelled);
                                }
                                self.queue.reload()?;
                                if self.queue.selected().as_ref() != Some(&prepared.item) {
                                    return Err(Error::QueueChanged);
                                }
                                transport.check()?;
                                self.equalizer_read()?;
                                if let Some(bypass) = transition
                                    && self.equalizer.status == EqualizerStatus::Configured
                                {
                                    self.equalizer_source_latch = bypass;
                                }
                            }
                            Ok(prepared)
                        }
                        .await
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(prepared) => {
                self.source_retry = None;
                self.position = Some(prepared.position);
                self.paused_seek = applied_seek.is_some() && pending.paused;
                if let Some(revision) = applied_seek {
                    self.seek_revision = revision;
                }
                self.reset = prepared.reset;
                self.state = if pending.paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                let mut podcast_progress = pending.podcast_progress;
                if let Some(capture) = &mut podcast_progress
                    && let Some(play_id) = self.play_id.as_deref()
                {
                    capture.reload(play_id);
                }
                let needs_skip_refresh =
                    prepared.audiobook_skip_sequence != self.audiobook_skip_sequence;
                let refresh_radio = needs_skip_refresh
                    .then(|| super::audiobook_skip::radio_id(&prepared.item))
                    .flatten();
                self.loaded = Some(Loaded {
                    audiobook_skip: (!needs_skip_refresh)
                        .then_some(prepared.audiobook_skip)
                        .flatten(),
                    history: pending.history,
                    podcast_progress,
                    item: prepared.item,
                    source: prepared.source,
                    media: prepared.media,
                    local_equalizer: prepared.local_equalizer,
                    timeline: prepared.timeline,
                });
                if let Some(radio) = refresh_radio {
                    self.refresh_audiobook_skip(self.token.clone(), radio);
                }
                let saved = self.save_position();
                if let Err(error) = &saved {
                    let _ = self.stop(false).await;
                    self.state = PlaybackStatus::Error;
                    self.record_failure(error);
                }
                if let Some(reply) = pending.reply {
                    self.reply(reply, saved);
                }
                let _ = self.schedule_prefetch();
            }
            Err(error) => {
                let _ = super::engine(self.engine.clone(), Default::default(), |engine| {
                    let stopped = engine.stop();
                    let cleared = engine.clear_loudness();
                    stopped.and(cleared)
                })
                .await;
                let _ = self.parametric_reset().await;
                self.state = PlaybackStatus::Error;
                self.record_failure(&error);
                if self.source_retry.is_none()
                    && let Some(position) = pending.authorized_position
                    && self.queue.selected().as_ref() == Some(&pending.item)
                    && self.ordinary_online_song(&pending.item)
                    && matches!(
                        self.error,
                        Some(
                            PlaybackFailure::OpenMedia
                                | PlaybackFailure::Output
                                | PlaybackFailure::Engine
                        )
                    )
                {
                    // Source authorization succeeded, but the new native load
                    // never became playable. Play can request a new URL for
                    // this entry without publishing an invented media clock.
                    self.source_retry = Some(SourceRetryPoint {
                        item: pending.item,
                        position,
                        paused: pending.paused,
                        history: pending.history,
                    });
                }
                self.loaded = None;
                self.loudness_forget_track();
                if let Some(reply) = pending.reply {
                    self.reply(reply, Err(error));
                }
            }
        }
    }
}
