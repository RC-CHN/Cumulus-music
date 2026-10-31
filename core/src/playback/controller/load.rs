use super::{
    Message, PlaybackStatus, Response,
    state::{Actor, Loaded, Pending},
};
use crate::{
    Error, Result,
    playback::{
        QueueEntry, Timeline, TimelineMode,
        engine::{LoadRequest, LoadedMedia},
        source::ResolvedSource,
    },
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct LoadOptions {
    pub paused: bool,
    pub position: Option<f64>,
    pub recover: bool,
    pub relative: bool,
    pub restart_at_end: bool,
    pub source: Option<Arc<ResolvedSource>>,
}
pub(super) struct Prepared {
    pub item: QueueEntry,
    pub source: Arc<ResolvedSource>,
    pub media: LoadedMedia,
    pub timeline: Timeline,
    pub position: f64,
    pub reset: bool,
}

impl Actor {
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
        // Recovery with an already authorized source must retain that source's
        // request. Ordinary loads resolve the latest account listening choice.
        if options.source.is_none() {
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
            .unwrap_or_default();
        if history.played_at.is_none() {
            self.history_error = None;
        }
        self.cancel_load().await?;
        self.loaded = None;
        self.loudness_forget_track();
        self.position = None;
        self.error = None;
        self.reset = false;
        self.play_id = Some(uuid::Uuid::new_v4().simple().to_string());
        self.state = PlaybackStatus::Preparing;
        let generation = self.generation();
        let cancel = self.guard();
        self.cancel = cancel.clone();
        self.pending = Some(Pending {
            history,
            id,
            item: item.clone(),
            reply: reply.take(),
            paused: options.paused,
        });
        let resolver = self.resolver.clone();
        let token = self.token.clone();
        let engine = self.engine.clone();
        let sender = self.sender.clone();
        self.load_task = Some(tokio::spawn(async move {
            let result = async {
                super::engine(engine.clone(), cancel.clone(), |engine| {
                    engine.stop()?;
                    // Discard the previous track's gain before priming a new decoder.
                    engine.clear_loudness()
                })
                .await?;
                cancel.check()?;
                let source = match options.source {
                    Some(source) => source,
                    None => resolver.resolve(&item.resource, &token).await?,
                };
                cancel.check()?;
                let timeline = crate::playback::PlaybackConstraint::apply(
                    constraint.as_ref(),
                    source.timeline(None)?,
                )?;
                let snapshot = timeline.snapshot();
                let resumed = if options.position.is_none() {
                    resolver
                        .voice_resume_position(&item.resource, &token)
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
                let _ = sender.send(Message::Loading(generation));
                let load_cancel = cancel.clone();
                let media = super::engine(engine.clone(), cancel.clone(), move |engine| {
                    engine.load(load, &load_cancel)
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
                Ok(Prepared {
                    item,
                    source,
                    media,
                    timeline,
                    position,
                    reset,
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
            self.error = Some(error.to_string());
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
        let result = match result {
            Ok(prepared) => {
                if let Err(error) = self.loudness_source(&prepared.source).await {
                    let _ = self.stop(false).await;
                    if let Some(reply) = pending.reply {
                        let _ = reply.send(Err(error));
                    }
                    return;
                }
                if self.equalizer.status == crate::effects::equalizer::EqualizerStatus::Pending
                    && let Err(error) = self.equalizer_apply().await
                {
                    let _ = self.stop(false).await;
                    if let Some(reply) = pending.reply {
                        let _ = reply.send(Err(error));
                    }
                    return;
                }
                let target = prepared.timeline.to_media(prepared.position);
                let prefs = self.prefs;
                let paused = pending.paused;
                match target {
                    Ok(target) => {
                        let initial = prepared.timeline.snapshot().media_start;
                        let result = super::engine(
                            self.engine.clone(),
                            self.cancel.clone(),
                            move |engine| {
                                if (target - initial).abs() > f64::EPSILON {
                                    engine.seek(target)?;
                                }
                                engine.volume(prefs.volume)?;
                                engine.speed(prefs.speed)?;
                                engine.pause(paused)
                            },
                        )
                        .await;
                        result.and_then(|_| {
                            self.queue.reload()?;
                            if self.queue.selected().as_ref() != Some(&prepared.item) {
                                return Err(Error::QueueChanged);
                            }
                            Ok(prepared)
                        })
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(prepared) => {
                self.position = Some(prepared.position);
                self.reset = prepared.reset;
                self.state = if pending.paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                self.loaded = Some(Loaded {
                    history: pending.history,
                    item: prepared.item,
                    source: prepared.source,
                    media: prepared.media,
                    timeline: prepared.timeline,
                });
                let saved = self.save_position();
                if let Err(error) = &saved {
                    let _ = self.stop(false).await;
                    self.state = PlaybackStatus::Error;
                    self.error = Some(error.to_string());
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
                self.state = PlaybackStatus::Error;
                self.error = Some(error.to_string());
                self.loaded = None;
                self.loudness_forget_track();
                if let Some(reply) = pending.reply {
                    self.reply(reply, Err(error));
                }
            }
        }
    }
}
