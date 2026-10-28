//! Native adjustments use the same actor fence, persistence and media ownership
//! as UI commands. A system surface cannot silently apply an old intent to new media.
use super::super::dto::{PlayerAction, VideoAction};
use super::*;
use crate::playback::PlaybackMode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeMediaAdjustment {
    Seek { seconds: f64 },
    Volume { value: f64 },
    Speed { value: f64 },
    Mode { mode: PlaybackMode },
}
impl MusicClient {
    pub async fn native_media_adjust(
        &self,
        target: &NativeMediaTarget,
        adjustment: NativeMediaAdjustment,
    ) -> Result<()> {
        target.check(self)?;
        // Validate before a rejected command can disturb an accepted transport.
        match adjustment {
            NativeMediaAdjustment::Seek { seconds } => {
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err(Error::InvalidInput("invalid native media position"));
                }
                let view = self.native_media_details()?;
                if view.transport.target.as_ref() != Some(target) {
                    return Err(Error::StaleResponse);
                }
                if !view.can_seek {
                    return Err(Error::State("native media is not seekable"));
                }
                if seconds < view.seek_start || view.seek_end.is_some_and(|end| seconds > end) {
                    return Err(Error::InvalidInput(
                        "native media position outside authorized range",
                    ));
                }
            }
            NativeMediaAdjustment::Volume { value }
                if !value.is_finite() || !(0.0..=100.0).contains(&value) =>
            {
                return Err(Error::InvalidInput("invalid native media volume"));
            }
            NativeMediaAdjustment::Speed { value }
                if !value.is_finite() || !(0.25..=4.0).contains(&value) =>
            {
                return Err(Error::InvalidInput("invalid native media speed"));
            }
            _ => (),
        }
        let epoch = target.token.generation.to_string();
        match target.media {
            Media::Audio(_) => {
                let action = match adjustment {
                    NativeMediaAdjustment::Seek { seconds } => PlayerAction::Seek { seconds },
                    NativeMediaAdjustment::Volume { value } => PlayerAction::Volume { value },
                    NativeMediaAdjustment::Speed { value } => PlayerAction::Speed { value },
                    NativeMediaAdjustment::Mode { mode } => PlayerAction::Mode { mode },
                };
                self.player_action_checked(&epoch, action, Some(target))
                    .await?;
            }
            Media::Video(_) => {
                let action = match adjustment {
                    NativeMediaAdjustment::Seek { seconds } => VideoAction::Seek { seconds },
                    NativeMediaAdjustment::Volume { value } => VideoAction::Volume { value },
                    NativeMediaAdjustment::Speed { value } => VideoAction::Speed { value },
                    NativeMediaAdjustment::Mode { .. } => {
                        return Err(Error::InvalidInput("MV has no transport queue"));
                    }
                };
                self.video_action_checked(&epoch, action, Some(target))
                    .await?;
            }
        }
        Ok(())
    }
}
