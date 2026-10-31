//! Context-bound mode changes. Delayed UI input cannot change a replaced queue.
use super::*;
use crate::playback::PlaybackMode;
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn player_mode_for(
        &self,
        epoch: &str,
        mode: PlaybackMode,
        revision: &str,
        fm: bool,
    ) -> Result<PlayerView> {
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid mode queue revision"))?;
        let token = self.player_token(epoch)?;
        // Room attachment and mode restrictions must not race each other.
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let action = PlayerAction::Mode { mode };
        self.together_validate_action(&token, &action)?;
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if before.queue_revision != revision || before.fm != fm {
            return Err(Error::QueueChanged);
        }
        if before
            .intelligence
            .as_ref()
            .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
            && self.focus.snapshot()?.owner != Some(super::super::focus::Owner::Audio)
        {
            return Err(Error::State("resume audio before leaving intelligence"));
        }
        let state = if fm {
            if !crate::playback::mode::ModeFamily::Fm.supports(mode) {
                return Err(Error::InvalidInput(
                    "FM only supports infinite or single repeat",
                ));
            }
            let repeat = if mode == PlaybackMode::SingleLoop {
                crate::fm::FmRepeat::One
            } else {
                crate::fm::FmRepeat::Infinite
            };
            self.player
                .fm()
                .await?
                .repeat_checked_for(
                    &token,
                    repeat,
                    self.player.selection.load(Ordering::Acquire),
                    before.fence(),
                )
                .await?
        } else if before.mode == mode
            && !before
                .intelligence
                .as_ref()
                .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
        {
            before
        } else {
            controller
                .command_guarded(
                    &token,
                    PlaybackCommand::Mode(mode),
                    before.fence(),
                    Cancellation::default(),
                )
                .await?
        };
        self.session.check_generation(&token)?;
        self.together_signal(&token, super::super::together::signal(&action));
        project(&state, &token)
    }
}
