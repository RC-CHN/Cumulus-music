use super::super::dto::{PlayerAction, VideoAction};
use super::super::player::NativeTransportPolicy;
use super::*;
impl MusicClient {
    /// System Play resumes paused media but starts stopped media at its
    /// authorized beginning, rather than the ordinary podcast/MV bookmark.
    pub async fn native_media_play(&self, target: &NativeMediaTarget) -> Result<()> {
        target.check(self)?;
        let epoch = target.token.generation.to_string();
        match target.media {
            Media::Audio(_) => {
                self.player_action_policy(
                    &epoch,
                    PlayerAction::Play,
                    Some(target),
                    NativeTransportPolicy::RestartStopped,
                )
                .await?;
            }
            Media::Video(_) => {
                self.video_action_with_restart(&epoch, VideoAction::Play, Some(target), true)
                    .await?;
            }
        }
        Ok(())
    }
    /// MPRIS stepping preserves transport state; tray/manual stepping retains
    /// its existing explicit-play behavior. The actor applies this atomically.
    pub async fn native_media_step(
        &self,
        target: &NativeMediaTarget,
        step: crate::playback::Step,
    ) -> Result<()> {
        target.check(self)?;
        if !matches!(target.media, Media::Audio(_)) {
            return Err(Error::InvalidInput("MV has no transport queue"));
        }
        let action = match step {
            crate::playback::Step::Next => PlayerAction::Next,
            crate::playback::Step::Previous => PlayerAction::Previous,
        };
        self.player_action_policy(
            &target.token.generation.to_string(),
            action,
            Some(target),
            NativeTransportPolicy::PreserveStep,
        )
        .await?;
        Ok(())
    }
    pub async fn native_media_action(
        &self,
        target: &NativeMediaTarget,
        action: NativeMediaAction,
    ) -> Result<()> {
        target.check(self)?;
        let action = if action == NativeMediaAction::Toggle {
            if target.playing {
                NativeMediaAction::Pause
            } else {
                NativeMediaAction::Play
            }
        } else {
            action
        };
        let epoch = target.token.generation.to_string();
        match target.media {
            Media::Audio(_) => {
                let action = match action {
                    NativeMediaAction::Play => PlayerAction::Play,
                    NativeMediaAction::Pause => PlayerAction::Pause { paused: true },
                    NativeMediaAction::Previous => PlayerAction::Previous,
                    NativeMediaAction::Next => PlayerAction::Next,
                    NativeMediaAction::Stop => PlayerAction::Stop,
                    NativeMediaAction::Toggle => unreachable!("resolved above"),
                };
                self.player_action_checked(&epoch, action, Some(target))
                    .await?;
            }
            Media::Video(_) => {
                let action = match action {
                    NativeMediaAction::Play => VideoAction::Play,
                    NativeMediaAction::Pause => VideoAction::Pause { paused: true },
                    NativeMediaAction::Stop => VideoAction::Stop,
                    NativeMediaAction::Previous | NativeMediaAction::Next => {
                        return Err(Error::InvalidInput("MV has no transport queue"));
                    }
                    NativeMediaAction::Toggle => unreachable!("resolved above"),
                };
                self.video_action_checked(&epoch, action, Some(target))
                    .await?;
            }
        }
        Ok(())
    }
}
