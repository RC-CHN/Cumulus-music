use super::{MusicClient, PlayerView, project};
use crate::{
    Error, Result,
    playback::{
        controller::{PlaybackCommand, PlaybackStatus, StartupRestore},
        preferences::StartupPreferences,
    },
};
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn startup_player(&self, epoch: &str) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let intent = self.player.selection.load(Ordering::Acquire);
        let audio_generation = self.focus.automatic_audio_generation()?;
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if !current.startup_pending || current.state != PlaybackStatus::Idle {
            return project(&current, &token);
        }
        let settings = self.session.with_local_owner(&token, |_| {
            StartupPreferences::read(&self.player.store)
                .map(|value| value.settings)
                .map_err(|error| match error {
                    Error::Json(_) => Error::State("playback settings damaged"),
                    other => other,
                })
        });
        let settings = match settings {
            Ok(settings) => settings,
            Err(error) => {
                // Repair and a later UI reconnect are not a new process launch.
                // This must not stop/pause a user action accepted in the meantime.
                let _ = controller
                    .command(&token, PlaybackCommand::SkipStartup(current.fence()))
                    .await;
                return Err(error);
            }
        };
        let lease = if current.startup_autoplay && settings.auto_play {
            match audio_generation {
                Some(expected) => Some(
                    self.claim_media_if(&token, super::super::focus::Owner::Audio, Some(expected))
                        .await?,
                ),
                None => None,
            }
        } else {
            None
        };
        if self.player.selection.load(Ordering::Acquire) != intent {
            return Err(Error::StaleResponse);
        }
        if let Some(lease) = lease {
            self.focus.check(lease, super::super::focus::Owner::Audio)?;
        }
        let input = StartupRestore {
            expected: current.fence(),
            queue: None,
            settings,
            allow_autoplay: lease.is_some(),
        };
        let state = if current.fm {
            self.player
                .fm()
                .await?
                .startup_for(&token, input, intent)
                .await?
        } else {
            controller
                .command(&token, PlaybackCommand::Startup(input))
                .await?
        };
        self.session.check_generation(&token)?;
        project(&state, &token)
    }
}
