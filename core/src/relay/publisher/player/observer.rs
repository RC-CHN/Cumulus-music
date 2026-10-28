use super::super::{PublicationEvent, PublicationFrame, record::NativeUpdate};
use super::{Runtime, events, project};
use crate::{
    Result,
    account::SessionToken,
    playback::{
        PlaybackQueue,
        controller::{PlaybackCommand, PlaybackStatus},
    },
    relay::{RelayResourceKind, VoiceProgressClock},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Hint {
    pub replace: bool,
    pub from_handoff: bool,
    pub seek: Option<f64>,
}
impl Hint {
    pub(crate) fn from_command(command: &PlaybackCommand) -> Self {
        Self {
            replace: matches!(command, PlaybackCommand::Replace { .. }),
            from_handoff: matches!(command,PlaybackCommand::UpdateQueue(update) if update.queue.items.iter().any(|item|item.source["scene"]=="playlistHandoff")),
            seek: if let PlaybackCommand::Seek(seconds) = command {
                Some(*seconds)
            } else {
                None
            },
        }
    }
}
pub(crate) struct Observer {
    pub runtime: Arc<Runtime>,
    token: SessionToken,
    queue_revision: u64,
    frame: PublicationFrame,
    first_played: bool,
    clock: VoiceProgressClock,
    interval: Option<u64>,
    origin: Instant,
    failed_at: Option<Instant>,
}
impl Observer {
    pub fn new(runtime: Arc<Runtime>, token: SessionToken, queue: &PlaybackQueue) -> Result<Self> {
        let frame = project(&queue.snapshot(), None, false);
        let failed = frame.is_err();
        runtime.recording(&token, failed)?;
        Ok(Self {
            runtime,
            token,
            queue_revision: queue.revision(),
            frame: frame.unwrap_or(PublicationFrame {
                queue_revision: 0,
                items: vec![],
                selected_entry: None,
                current: None,
                source: None,
                mode: Some(queue.mode().into()),
                eligible_scene: false,
            }),
            first_played: false,
            clock: Default::default(),
            interval: None,
            origin: Instant::now(),
            failed_at: failed.then(Instant::now),
        })
    }
    pub fn bind(&mut self, token: &SessionToken, queue: &PlaybackQueue) {
        if &self.token == token {
            return;
        }
        self.token = token.clone();
        self.queue_revision = queue.revision();
        self.first_played = false;
        self.clock.reset();
        self.interval = None;
        match project(&queue.snapshot(), None, false) {
            Ok(frame) => {
                self.frame = frame;
                self.failed_at = None;
                let _ = self.runtime.recording(token, false);
            }
            Err(_) => {
                self.failed_at = Some(Instant::now());
                let _ = self.runtime.recording(token, true);
            }
        }
    }
    /// Invoked after every serialized actor transition, including automatic EOF
    /// and external queue reloads. No HTTP await enters the playback actor.
    pub fn observe(
        &mut self,
        token: &SessionToken,
        queue: &PlaybackQueue,
        status: PlaybackStatus,
        position: Option<f64>,
        hint: Hint,
    ) {
        if let Err(_error) = self.apply(token, queue, status, position, hint) {
            self.failed_at = Some(Instant::now());
            let _ = self.runtime.recording(token, true);
        }
    }
    fn apply(
        &mut self,
        token: &SessionToken,
        queue: &PlaybackQueue,
        status: PlaybackStatus,
        position: Option<f64>,
        hint: Hint,
    ) -> Result<()> {
        let rebound = &self.token != token;
        if rebound {
            self.token = token.clone();
            self.queue_revision = queue.revision();
            self.frame = project(&queue.snapshot(), None, false)?;
            self.first_played = false;
            self.clock.reset();
            self.interval = None;
            self.failed_at = None;
            self.runtime.recording(token, false)?;
        }
        if token.uid.is_none() {
            return Ok(());
        }
        if self
            .failed_at
            .is_some_and(|when| when.elapsed() < Duration::from_secs(5))
        {
            return Ok(());
        }
        let changed = self.queue_revision != queue.revision() || hint.replace;
        let next = if changed || self.failed_at.is_some() {
            Some(project(&queue.snapshot(), Some(&self.frame), hint.replace)?)
        } else {
            None
        };
        let candidate = next.as_ref().unwrap_or(&self.frame);
        let publisher = &self.runtime.publisher;
        let mut wrote = false;
        if self.failed_at.is_some() {
            publisher.record_native(token, candidate.clone(), NativeUpdate::Repair)?;
            wrote = true;
        } else {
            let mut changes = if changed {
                events(&self.frame, candidate, hint.replace, hint.from_handoff)
            } else {
                vec![]
            };
            if let Some(seconds) = hint.seek
                && candidate
                    .current
                    .as_ref()
                    .is_some_and(|r| r.kind == RelayResourceKind::Voice)
            {
                changes.push(PublicationEvent::VoiceProgress {
                    progress_ms: milliseconds(seconds)?,
                });
            }
            if !changes.is_empty() {
                publisher.record_native(token, candidate.clone(), NativeUpdate::Events(changes))?;
                wrote = true;
            }
        }
        if status == PlaybackStatus::Playing && !self.first_played {
            publisher.record_native(token, candidate.clone(), NativeUpdate::Resume)?;
            self.first_played = true;
            wrote = true;
        }
        let config = publisher.live_config(token)?;
        let interval = config
            .as_ref()
            .filter(|c| c.enable)
            .map(|c| c.radio_progress_interval);
        if interval != self.interval {
            self.interval = interval;
            self.clock.reset();
        }
        if let Some(interval) = interval {
            let voice = candidate
                .current
                .as_ref()
                .is_some_and(|r| r.kind == RelayResourceKind::Voice);
            if let Some(progress) = self.clock.sample(
                self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64,
                interval,
                milliseconds(position.unwrap_or(0.0))?,
                status == PlaybackStatus::Playing,
                voice,
            ) {
                publisher.record_native(
                    token,
                    candidate.clone(),
                    NativeUpdate::Events(vec![PublicationEvent::VoiceProgress {
                        progress_ms: progress,
                    }]),
                )?;
                wrote = true;
            }
        }
        if let Some(frame) = next {
            self.frame = frame;
        }
        self.queue_revision = queue.revision();
        self.failed_at = None;
        self.runtime.recording(token, false)?;
        if wrote {
            self.runtime.wake();
        }
        Ok(())
    }
}
fn milliseconds(seconds: f64) -> Result<u64> {
    if !seconds.is_finite() || seconds < 0.0 || seconds > u64::MAX as f64 / 1000.0 {
        return Err(crate::Error::InvalidInput("invalid relay progress"));
    }
    Ok((seconds * 1000.0).round() as u64)
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.runtime.stop();
    }
}
