use super::{
    Message, PlaybackStatus, PrefetchSnapshot,
    load::{LoadOptions, LoadSource},
    state::{Actor, SourceRetryPoint},
};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        PlaybackQueue, Step,
        engine::{Cancellation, EndReason, EngineEvent},
        failure::PlaybackFailure,
    },
};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PrefetchKey {
    generation: u64,
    queue_revision: u64,
    source_revision: u64,
    quality_revision: String,
    entry_id: String,
    token: SessionToken,
}
pub(super) struct Prefetched {
    key: PrefetchKey,
    result: Result<()>,
    deadline: Option<Duration>,
}
impl Actor {
    pub async fn rebind(&mut self) -> Result<()> {
        let token = self.session.token()?;
        if token == self.token && self.bound {
            self.parametric_retry_reset().await?;
            self.parametric_supervise().await?;
            self.quality_sync()?;
            self.equalizer_sync().await?;
            return self.loudness_sync().await;
        }
        let carry_startup_autoplay = self.startup_pending
            && self.startup_autoplay
            && self.session.is_initial_restore(&self.token, &token)?;
        let stopped = self.stop(false).await;
        // Never bind a new owner while the old native component may be live.
        self.parametric_reset().await?;
        self.clear_parametric_failure_notice();
        let owner = token.uid.as_deref().unwrap_or("local-guest");
        // Clear the previous account even if the new account's database is
        // corrupt. An unbound actor retries loading; it cannot write a fallback.
        self.queue = PlaybackQueue::new(None, owner)?;
        self.prefs = Default::default();
        self.quality_preferences = Default::default();
        self.token = token;
        self.bound = false;
        self.startup_pending = true;
        self.startup_autoplay = carry_startup_autoplay;
        self.history_error = None;
        self.equalizer_clear().await?;
        self.resolver.invalidate(None)?;
        let owner = self.token.uid.as_deref().unwrap_or("local-guest");
        let queue = PlaybackQueue::new(Some(self.store.clone()), owner)?;
        let prefs = Self::preferences(&self.store, owner)?;
        self.queue = queue;
        self.prefs = prefs;
        self.bound = true;
        if let Some(relay) = &mut self.relay {
            relay.bind(&self.token, &self.queue);
        }
        self.error = None;
        self.equalizer_sync().await?;
        self.loudness_sync().await?;
        self.quality_sync()?;
        stopped
    }
    pub async fn sync_queue(&mut self) -> Result<()> {
        self.sync_queue_after_edit(None).await
    }

    pub(super) async fn sync_queue_after_edit(
        &mut self,
        resume_after_removal: Option<Cancellation>,
    ) -> Result<()> {
        self.queue.reload()?;
        if self
            .source_retry
            .as_ref()
            .is_some_and(|retry| self.queue.selected().as_ref() != Some(&retry.item))
        {
            self.source_retry = None;
        }
        let before_item = self
            .loaded
            .as_ref()
            .map(|l| &l.item)
            .or_else(|| self.pending.as_ref().map(|p| &p.item));
        if before_item.is_some() && before_item != self.queue.selected().as_ref() {
            let position = self.position;
            let before = before_item.map(|item| item.entry_id.clone());
            let after = self.queue.selected();
            let was_paused = self.state == PlaybackStatus::Paused
                || self.pending.as_ref().is_some_and(|p| p.paused);
            self.stop(false).await?;
            if let Some(after) = after {
                let paused =
                    was_paused && resume_after_removal.is_none_or(|owner| owner.is_cancelled());
                self.begin(
                    LoadOptions {
                        paused,
                        position: if before.as_ref() == Some(&after.entry_id) {
                            position
                        } else {
                            None
                        },
                        recover: true,
                        ..Default::default()
                    },
                    uuid::Uuid::new_v4().simple().to_string(),
                    None,
                )
                .await?;
            }
        }
        Ok(())
    }
    pub async fn tick(&mut self) -> Result<()> {
        self.rebind().await?;
        self.sync_queue().await?;
        if !self.state.active() {
            if self.pending.is_none() {
                super::engine(self.engine.clone(), self.guard(), |engine| {
                    engine.maintain_output()
                })
                .await?;
            }
            return Ok(());
        }
        self.equalizer_observe().await?;
        self.loudness_observe().await?;
        let view = self.poll_with_effect_recovery().await?;
        let loaded = self
            .loaded
            .as_ref()
            .ok_or(Error::State("active playback missing media"))?;
        // Native clock and pause/cache flags describe only the reported entry.
        // End events are checked separately, including after the entry closes.
        let current_entry = view.entry_id == Some(loaded.media.entry_id);
        self.parametric.observe_samples(
            loaded.media.entry_id,
            view.parametric_samples
                .filter(|_| current_entry && !view.paused && !view.buffering && !view.seeking),
        );
        if current_entry {
            self.native_seeking = view.seeking;
        }
        let reported_position = view.position_for(loaded.media.entry_id);
        let current_position = reported_position.filter(|_| !self.paused_seek);
        let mut events = std::mem::take(&mut self.deferred_events);
        events.extend(view.events);
        let was_paused = self.state == PlaybackStatus::Paused;
        // Native events can share a poll (including ones drained by a
        // transport command). A dead engine or current-entry failure cannot
        // be converted into successful EOF or a fresh load by another event.
        let failure = if events.iter().any(|event| match event {
            EngineEvent::Shutdown => true,
            EngineEvent::Ended {
                entry_id,
                reason: EndReason::Quit,
                ..
            } => *entry_id == loaded.media.entry_id,
            _ => false,
        }) {
            Some(PlaybackFailure::Engine)
        } else {
            events.iter().rev().find_map(|event| match event {
                EngineEvent::Ended {
                    entry_id,
                    reason: EndReason::Error | EndReason::Redirect | EndReason::Unknown(_),
                    error,
                } if *entry_id == loaded.media.entry_id => {
                    Some(PlaybackFailure::from_error(&Error::Playback {
                        operation: "stream ended",
                        code: *error,
                    }))
                }
                _ => None,
            })
        };
        if failure.is_none()
            && events.iter().any(|event| {
                matches!(event, EngineEvent::OutputReload { entry_id } if *entry_id == loaded.media.entry_id)
            })
        {
            let observed = current_position
                .map(|p| loaded.timeline.from_media(p))
                .transpose()?;
            let position = match (was_paused, self.position, observed) {
                (true, Some(accepted), Some(observed)) => Some(accepted.max(observed)),
                _ => observed.or(self.position),
            };
            let options = LoadOptions {
                paused: self.state == PlaybackStatus::Paused,
                position,
                recover: true,
                retain_source_retry: position.is_some(),
                source: if self.ordinary_online_song(&loaded.item) {
                    LoadSource::RenewIfExpired(loaded.source.clone())
                } else {
                    LoadSource::Retained(loaded.source.clone())
                },
                ..Default::default()
            };
            return self.begin(options, uuid::Uuid::new_v4().simple().to_string(), None).await;
        }
        // A matching native stop is terminal even when the player didn't
        // issue Stop itself. Never treat it as successful completion.
        let stopped = failure.is_none()
            && events.iter().any(|event| {
                matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Stop, .. }
                if *entry_id == loaded.media.entry_id)
            });
        let ended = failure.is_none()
            && !stopped
            && events.iter().any(|event| {
                matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Eof, .. }
                if *entry_id == loaded.media.entry_id)
            });
        let failed = failure.is_some();
        if let Some(failure) = failure {
            self.state = PlaybackStatus::Error;
            self.error = Some(failure);
        } else if ended {
            self.state = PlaybackStatus::Ended;
        }
        if current_entry
            && matches!(
                self.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering
            )
        {
            self.state = if view.paused {
                PlaybackStatus::Paused
            } else if view.buffering {
                PlaybackStatus::Buffering
            } else {
                PlaybackStatus::Playing
            };
        }
        if ended {
            self.position = loaded.timeline.display_end();
        } else if let Some(position) = current_position {
            // A paused libmpv clock can include queued output latency, which
            // changes again when speed changes. Keep an accepted paused seek
            // from rewinding the UI or durable restore. Accept
            // forward observations, including Together preview/progress clocks.
            let observed = loaded.timeline.from_media(position)?;
            if !was_paused || !view.paused || !self.position.is_some_and(|p| observed < p) {
                self.position = Some(observed);
            }
        }
        if stopped {
            let retry = self
                .position
                .filter(|_| self.independent_transport(&loaded.item))
                .map(|position| SourceRetryPoint {
                    item: loaded.item.clone(),
                    position,
                    paused: was_paused,
                    history: loaded.history.clone(),
                });
            // Save the accepted clock as stopped, not as a completed track,
            // then release the old decoder and its transport identity.
            self.state = PlaybackStatus::Idle;
            self.stop(true).await?;
            self.error = None;
            self.source_retry = retry;
            return Ok(());
        }
        self.buffered_through = if !ended && reported_position.is_some() {
            view.buffered_through
                .and_then(|time| loaded.timeline.from_media(time).ok())
                .filter(|end| self.position.is_some_and(|position| *end > position))
        } else {
            None
        };
        if failed {
            // Error ends active polling. Save the last accepted clock now and
            // keep the previous transport intent for an explicit same-entry retry.
            self.source_retry = self.position.map(|position| SourceRetryPoint {
                item: loaded.item.clone(),
                position,
                paused: was_paused,
                history: loaded.history.clone(),
            });
        }
        if failed || ended || self.last_save.elapsed() >= Duration::from_secs(1) {
            if failed || ended {
                self.parametric.discard_paused_device();
            }
            self.save_position()?;
        }
        let advance =
            ended && !self.queue.is_fm() && self.queue.advance(Step::Next, true)?.is_some();
        if ended && (!advance || self.queue.mode() != crate::playback::PlaybackMode::Sequential) {
            self.power_set(crate::playback::keep_awake::OfficialPlayingState::End);
        }
        if advance {
            self.begin(
                LoadOptions {
                    // EOF may have been drained by an explicit pause.
                    // Carry the user's pause through the automatic step.
                    paused: was_paused,
                    ..Default::default()
                },
                uuid::Uuid::new_v4().simple().to_string(),
                None,
            )
            .await?;
        }
        self.schedule_prefetch()
    }
    pub fn schedule_prefetch(&mut self) -> Result<()> {
        if !self.state.active() {
            self.prefetch_key = None;
            self.prefetch = PrefetchSnapshot {
                state: "idle",
                entry_id: None,
            };
            return Ok(());
        }
        let Some(mut next) = self.queue.peek(Step::Next, true).filter(|next| {
            next.resource.get("id").is_some()
                && next.resource.get("local_id").is_none()
                && next.resource.get("path").is_none()
        }) else {
            self.prefetch_key = None;
            self.refresh_at = None;
            self.prefetch = PrefetchSnapshot {
                state: "idle",
                entry_id: None,
            };
            return Ok(());
        };
        self.apply_quality_preference(&mut next.resource);
        let key = PrefetchKey {
            generation: self.generation(),
            queue_revision: self.queue.revision(),
            source_revision: self.resolver.revision()?,
            quality_revision: self.quality_preferences.preferences.revision.clone(),
            entry_id: next.entry_id.clone(),
            token: self.token.clone(),
        };
        if self.prefetch_task.is_some() {
            if self.prefetch_key.as_ref() != Some(&key) {
                self.prefetch_key = None;
                self.refresh_at = None;
                self.prefetch = PrefetchSnapshot {
                    state: "idle",
                    entry_id: None,
                };
            }
            return Ok(());
        }
        if self.prefetch_key.as_ref() == Some(&key)
            && self
                .refresh_at
                .is_none_or(|deadline| deadline > self.resolver.now())
        {
            return Ok(());
        }
        self.prefetch_key = Some(key.clone());
        self.refresh_at = None;
        self.prefetch = PrefetchSnapshot {
            state: "resolving",
            entry_id: Some(next.entry_id),
        };
        let resolver = self.resolver.clone();
        let sender = self.sender.clone();
        let token = self.token.clone();
        let resource: Value = next.resource;
        self.prefetch_task = Some(tokio::spawn(async move {
            let result = resolver.resolve(&resource, &token).await.map(|_| ());
            let deadline = resolver
                .cached_until(&resource, &token)
                .ok()
                .flatten()
                .map(|d| d.max(resolver.now() + Duration::from_secs(5)));
            let _ = sender.send(Message::Prefetched(Prefetched {
                key,
                result,
                deadline,
            }));
        }));
        Ok(())
    }
    pub fn prefetched(&mut self, value: Prefetched) {
        self.prefetch_task = None;
        if self.prefetch_key.as_ref() == Some(&value.key)
            && self.generation() == value.key.generation
            && self.token == value.key.token
            && self.session.token().ok().as_ref() == Some(&value.key.token)
            && self.queue.revision() == value.key.queue_revision
            && self.resolver.revision().ok() == Some(value.key.source_revision)
            && self.quality_preferences.preferences.revision == value.key.quality_revision
        {
            self.refresh_at = value.deadline;
            self.prefetch = PrefetchSnapshot {
                state: if value.result.is_ok() {
                    "ready"
                } else {
                    "error"
                },
                entry_id: Some(value.key.entry_id),
            };
        }
    }
}
