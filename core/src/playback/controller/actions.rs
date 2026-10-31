use super::{PlaybackCommand, PlaybackStatus, Reply, Request, load::LoadOptions, state::Actor};
use crate::{Error, Result, playback::Step};

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
        if matches!(request.command, PlaybackCommand::Queue) {
            let _ = request.reply.send(Ok(Reply {
                state: self.snapshot(),
                queue: Some(self.queue.snapshot()),
            }));
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
                | PlaybackCommand::Equalizer { .. }
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
                    | PlaybackCommand::Reorder(_)
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
        if let PlaybackCommand::UpdateQueue(value) = &request.command
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
                | PlaybackCommand::Equalizer { .. }
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
        if matches!(
            request.command,
            PlaybackCommand::Replace { .. }
                | PlaybackCommand::Select(_)
                | PlaybackCommand::Next
                | PlaybackCommand::Previous
                | PlaybackCommand::StepPreservingState(_)
                | PlaybackCommand::Remove(_)
        ) && let Err(error) = self.save_position()
        {
            self.reply(request.reply, Err(error));
            return;
        }
        let relay_hint = crate::relay::publisher::player::Hint::from_command(&request.command);
        let result = match request.command {
            PlaybackCommand::Startup(input) => {
                self.startup_restore(input).map(|value| options = value)
            }
            PlaybackCommand::Equalizer {
                expected_revision,
                settings,
            } => self.equalizer_change(expected_revision, settings).await,
            PlaybackCommand::RetryEqualizer => self.equalizer_retry().await,
            PlaybackCommand::RepairEqualizer => self.equalizer_repair().await,
            PlaybackCommand::Loudness {
                expected_revision,
                enabled,
            } => self.loudness_change(&expected_revision, enabled).await,
            PlaybackCommand::RetryLoudness => self.loudness_retry().await,
            PlaybackCommand::RepairLoudness => self.loudness_repair().await,
            PlaybackCommand::RepairQuality => self.quality_repair(),
            PlaybackCommand::UpdateQueue(value) => {
                self.update_queue(value).await.map(|value| options = value)
            }
            PlaybackCommand::ApplyRemote(value) => {
                self.apply_remote(value).await.map(|value| options = value)
            }
            PlaybackCommand::Replace {
                mut resources,
                index,
                source,
            } => {
                self.stamp_quality(&mut resources);
                self.queue.replace(resources, index, source).map(|_| {
                    if !self.queue.is_empty() {
                        options = Some(LoadOptions::default());
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
            PlaybackCommand::Select(id) => self
                .queue
                .select(&id)
                .map(|_| options = Some(LoadOptions::default())),
            PlaybackCommand::Remove(ids) => self.queue.remove(&ids),
            PlaybackCommand::Reorder(ids) => self.queue.reorder(&ids),
            PlaybackCommand::Mode(mode) => self.queue.set_mode(mode),
            PlaybackCommand::PlayFromStart => {
                if self.state == PlaybackStatus::Paused || self.pending.is_some() {
                    self.pause(false).await
                } else if self.state.active() {
                    Ok(())
                } else {
                    options = Some(LoadOptions {
                        position: Some(0.0),
                        relative: true,
                        ..Default::default()
                    });
                    Ok(())
                }
            }
            PlaybackCommand::Play => {
                if self.state == PlaybackStatus::Paused {
                    self.pause(false).await
                } else if self.state.active() {
                    Ok(())
                } else {
                    options = Some(LoadOptions::default());
                    Ok(())
                }
            }
            PlaybackCommand::Pause(paused) => self.pause(paused).await,
            PlaybackCommand::Seek(seconds) => self.seek(seconds).await,
            PlaybackCommand::Volume(value) => self.preference(value, true).await,
            PlaybackCommand::Speed(value) => self.preference(value, false).await,
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
                    Ok(None) => self.stop(true).await,
                    Err(error) => Err(error),
                }
            }
            PlaybackCommand::Stop => self.stop(true).await,
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
                            options = Some(LoadOptions {
                                paused: self.state == PlaybackStatus::Paused,
                                position: self.position,
                                recover: true,
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
            PlaybackCommand::Snapshot | PlaybackCommand::SyncQueue => Ok(()),
            PlaybackCommand::Quality(_)
            | PlaybackCommand::TrialQuality { .. }
            | PlaybackCommand::Queue
            | PlaybackCommand::SkipStartup(_) => unreachable!("handled above"),
        };
        if let Err(error) = result {
            self.reply(request.reply, Err(error));
            return;
        }
        self.relay_hint = relay_hint;
        if let Some(options) = options {
            if let Err(error) = self.begin(options, request.id, Some(request.reply)).await {
                self.error = Some(error.to_string());
            }
        } else {
            let result = self.sync_queue().await;
            self.reply(request.reply, result);
        }
    }
    fn require_loaded(&self) -> Result<()> {
        if self.state.active() && self.loaded.is_some() {
            Ok(())
        } else {
            Err(Error::State("nothing is loaded"))
        }
    }
    async fn pause(&mut self, paused: bool) -> Result<()> {
        if let Some(pending) = &mut self.pending {
            // A pause accepted during remote/ordinary decoding must determine
            // its first presentation state, rather than fail and later autoplay.
            pending.paused = paused;
            return Ok(());
        }
        self.require_loaded()?;
        let preserve_position = paused && self.state == PlaybackStatus::Paused;
        let view = super::engine(self.engine.clone(), self.guard(), move |engine| {
            engine.pause(paused)?;
            engine.poll()
        })
        .await?;
        if let Some(position) = view.position {
            let observed = self
                .loaded
                .as_ref()
                .expect("checked")
                .timeline
                .from_media(position)?;
            if !preserve_position || !self.position.is_some_and(|p| observed < p) {
                self.position = Some(observed);
            }
        }
        self.state = if paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        };
        self.persist_transport().await
    }
    async fn seek(&mut self, seconds: f64) -> Result<()> {
        self.require_loaded()?;
        let media = self
            .loaded
            .as_ref()
            .expect("checked")
            .timeline
            .to_media(seconds)?;
        super::engine(self.engine.clone(), self.guard(), move |engine| {
            engine.seek(media)
        })
        .await?;
        self.position = Some(seconds);
        self.seek_revision = self.control_revision;
        self.persist_transport().await
    }
    pub(super) async fn persist_transport(&mut self) -> Result<()> {
        if let Err(error) = self.save_position() {
            let _ = self.stop(false).await;
            self.state = PlaybackStatus::Error;
            self.error = Some(error.to_string());
            return Err(error);
        }
        Ok(())
    }
    async fn preference(&mut self, value: f64, volume: bool) -> Result<()> {
        if !value.is_finite()
            || (volume && !(0.0..=100.0).contains(&value))
            || (!volume && !(0.25..=4.0).contains(&value))
        {
            return Err(Error::InvalidInput("invalid player preference"));
        }
        let before = self.prefs;
        let mut next = before;
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
            next.speed = value;
        }
        if self.state.active() {
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
            if self.state.active() {
                super::engine(self.engine.clone(), self.guard(), move |engine| {
                    if volume {
                        engine.volume(before.volume)
                    } else {
                        engine.speed(before.speed)
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
