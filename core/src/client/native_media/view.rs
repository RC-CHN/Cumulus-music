use super::*;
use crate::{mv::playback::VideoStatus, playback::controller::PlaybackStatus};
impl MusicClient {
    /// Read existing actors only: no decoder initialization, network or account mutation.
    pub fn native_media_snapshot(&self) -> Result<NativeMediaSnapshot> {
        let token = self.session.token()?;
        let focus = self.focus.snapshot()?;
        let value = if focus.owner == Some(Owner::Video) {
            let Some(controller) = self.mv.existing(&token)? else {
                return Ok(NativeMediaSnapshot::default());
            };
            let value = controller.snapshot()?;
            if value.id.is_none() {
                return Ok(NativeMediaSnapshot::default());
            }
            let playing = matches!(value.status, VideoStatus::Playing)
                || matches!(value.status, VideoStatus::Resolving | VideoStatus::Loading)
                    && !value.paused;
            NativeMediaSnapshot {
                title: value
                    .detail
                    .as_ref()
                    .map(|d| d.title.clone())
                    .unwrap_or_else(|| "MV".into()),
                playing,
                can_previous: false,
                can_next: false,
                target: Some(NativeMediaTarget {
                    token: token.clone(),
                    focus,
                    media: Media::Video(value.fence()),
                    playing,
                }),
            }
        } else {
            let Some(controller) = self.player.controller.get() else {
                return Ok(NativeMediaSnapshot::default());
            };
            let value = controller.snapshot();
            if value.account != token || value.selected.is_none() {
                return Ok(NativeMediaSnapshot::default());
            }
            let public = super::super::player::project(&value, &token)?;
            let playing = matches!(
                value.state,
                PlaybackStatus::Playing
                    | PlaybackStatus::Preparing
                    | PlaybackStatus::Loading
                    | PlaybackStatus::Buffering
            );
            NativeMediaSnapshot {
                title: public.track.map(|track| track.name).unwrap_or_default(),
                playing,
                can_previous: true,
                can_next: true,
                target: Some(NativeMediaTarget {
                    token: token.clone(),
                    focus,
                    media: Media::Audio(value.fence()),
                    playing,
                }),
            }
        };
        self.session.check_generation(&token)?;
        self.focus.check_snapshot(focus)?;
        Ok(value)
    }
}
