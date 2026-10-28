use super::{PlaybackStatus, RemotePlayback, load::LoadOptions, state::Actor};
use crate::{Error, Result};

impl Actor {
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
        let result = super::engine(self.engine.clone(), self.guard(), move |engine| {
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
            self.error = Some(error.to_string());
            return Err(error);
        }
        self.position = Some(value.position);
        self.state = if value.paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        };
        self.persist_transport().await?;
        Ok(None)
    }
}
