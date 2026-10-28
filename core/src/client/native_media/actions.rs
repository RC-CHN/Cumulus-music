use super::super::dto::{PlayerAction, VideoAction};
use super::*;
impl MusicClient {
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
